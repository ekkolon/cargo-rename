mod common;

use std::fs;

use common::*;

use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_package_not_found() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    run_rename(workspace_root, "nonexistent-crate", "new-name", &[])
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_path_traversal_attempts() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Reject .. in path
    run_rename(
        workspace_root,
        "crate-a",
        "evil-crate",
        &["--move", "../../../etc/passwd"],
    )
    .failure()
    .stderr(predicate::str::contains("Contains '..'"));

    // Reject ../ pattern
    run_rename(
        workspace_root,
        "crate-a",
        "evil-crate",
        &["--move", "../../outside"],
    )
    .failure()
    .stderr(predicate::str::contains("Contains '..'"));

    // Reject path starting with ..
    run_rename(
        workspace_root,
        "crate-a",
        "evil-crate",
        &["--move", "../sibling"],
    )
    .failure()
    .stderr(predicate::str::contains("Contains '..'"));
}

#[test]
fn test_dot_paths_rejected() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Reject "."
    run_rename(workspace_root, "crate-a", "new-name", &["--move", "."])
        .failure()
        .stderr(predicate::str::contains("Cannot use '.' or '..'"));

    // Reject ".."
    run_rename(workspace_root, "crate-a", "new-name", &["--move", ".."])
        .failure()
        .stderr(predicate::str::contains("Cannot use '.' or '..'"));
}

#[test]
fn test_absolute_paths_outside_workspace_warns() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Create another temp directory (simulating external workspace)
    let external = TempDir::new().unwrap();
    let external_path = external.path().join("external-location");

    // DON'T create the directory - let the tool create it

    // Absolute path outside workspace - tool allows it but warns
    // However, Cargo will reject the workspace afterward
    let result = run_rename(
        workspace_root,
        "crate-a",
        "external-crate",
        &["--move", external_path.to_str().unwrap()],
    );

    // The tool completes the rename but warns about workspace issues
    // Check that it either warned or showed workspace verification error
    result.code(0).stderr(
        predicate::str::contains("Warning: Using absolute path outside workspace")
            .or(predicate::str::contains("Workspace verification failed")),
    );
}

#[test]
#[cfg(windows)]
fn test_unc_paths_rejected() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // UNC paths should fail (either validation or filesystem error)
    run_rename(
        workspace_root,
        "crate-a",
        "evil-crate",
        &["--move", r"\\server\share"],
    )
    .failure()
    .stderr(
        predicate::str::contains("UNC paths are not allowed")
            .or(predicate::str::contains("outside workspace"))
            .or(predicate::str::contains("failed to create")),
    );
}

#[test]
fn test_same_name_without_move_fails() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    run_rename(workspace_root, "crate-a", "crate-a", &[])
        .failure()
        .stderr(predicate::str::contains("same as").or(predicate::str::contains("Nothing to do")));
}

#[test]
fn test_target_directory_exists() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Create backend directory
    fs::create_dir(workspace_root.join("backend")).unwrap();

    // Create the EXACT target path that will conflict
    let target_crate = workspace_root.join("backend/moved-crate");
    fs::create_dir_all(&target_crate).unwrap();
    fs::write(
        target_crate.join("Cargo.toml"),
        "[package]\nname = \"dummy\"\n",
    )
    .unwrap();

    run_rename(
        workspace_root,
        "crate-a",
        "moved-crate",
        &["--move", "backend"],
    )
    .failure()
    .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_invalid_package_names() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Empty name - clap will reject this
    run_rename(workspace_root, "crate-a", "", &[]).failure();

    // Name with spaces
    run_rename(workspace_root, "crate-a", "my crate", &[])
        .failure()
        .stderr(predicate::str::contains("Invalid package name"));

    // Name starting with number
    run_rename(workspace_root, "crate-a", "123-crate", &[])
        .failure()
        .stderr(predicate::str::contains("Invalid package name"));
}

#[test]
#[cfg(windows)]
fn test_windows_reserved_names() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Reserved device names
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "LPT1"];

    for name in &reserved {
        run_rename(workspace_root, "crate-a", "moved-crate", &["--move", name])
            .failure()
            .stderr(predicate::str::contains("Windows reserved name"));
    }
}

#[test]
#[cfg(windows)]
fn test_windows_invalid_characters() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let invalid_chars = ["<", ">", ":", "\"", "|", "?", "*"];

    for ch in &invalid_chars {
        let path = format!("bad{}dir", ch);
        run_rename(workspace_root, "crate-a", "moved-crate", &["--move", &path])
            .failure()
            .stderr(predicate::str::contains("contains invalid char"));
    }
}

#[test]
fn test_relative_paths_work() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // DON'T create backend directory - let the tool create it
    // Or if tool requires it to exist, create it but ensure target doesn't exist

    // If your tool checks that the directory exists, create it:
    // fs::create_dir(workspace_root.join("backend")).unwrap();

    // Simple relative path - should create backend/moved-crate
    run_rename(
        workspace_root,
        "crate-a",
        "moved-crate",
        &["--move", "backend"],
    )
    .success();

    // Verify crate was moved to backend/moved-crate
    assert!(
        workspace_root.join("backend/Cargo.toml").exists(),
        "Expected backend/Cargo.toml to exist"
    );
}

#[test]
fn test_nested_relative_paths() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // DON'T create the nested structure - your tool rejects existing directories
    // Let the tool create the entire path

    run_rename(
        workspace_root,
        "crate-a",
        "deeply-nested",
        &["--move", "backend/services/api/internal/deeply-nested"],
    )
    .success();

    assert!(
        workspace_root
            .join("backend/services/api/internal/deeply-nested/Cargo.toml")
            .exists(),
        "Expected deeply-nested crate in backend/services/api/internal"
    );
}

#[test]
#[cfg(windows)]
fn test_windows_drive_letter_paths() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // DON'T create services directory - tool will reject if it exists

    // Use absolute path to services directory
    let target = workspace_root.join("services/windows-crate");

    run_rename(
        workspace_root,
        "crate-a",
        "windows-crate",
        &["--move", target.to_str().unwrap()],
    )
    .success();

    // Should create services/windows-crate
    assert!(
        target.join("Cargo.toml").exists(),
        "Expected services/windows-crate/Cargo.toml"
    );
}

#[test]
#[cfg(unix)]
fn test_unix_absolute_paths() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let target = workspace_root.join("services");

    run_rename(
        workspace_root,
        "crate-a",
        "unix-crate",
        &["--move", target.to_str().unwrap()],
    )
    .success();

    assert!(
        target.join("unix-crate/Cargo.toml").exists(),
        "Expected services/unix-crate/Cargo.toml"
    );
}
