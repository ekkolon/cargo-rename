use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn create_test_workspace() -> TempDir {
    let temp = TempDir::new().unwrap();

    let workspace_toml = r#"
[workspace]
members = ["crate-a", "crate-b"]
resolver = "2"
"#;
    fs::write(temp.path().join("Cargo.toml"), workspace_toml).unwrap();

    let crate_a = temp.path().join("crate-a");
    fs::create_dir(&crate_a).unwrap();
    fs::write(
        crate_a.join("Cargo.toml"),
        r#"
[package]
name = "crate-a"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    fs::create_dir(crate_a.join("src")).unwrap();
    fs::write(
        crate_a.join("src/lib.rs"),
        r#"pub fn hello() -> &'static str { "Hello" }"#,
    )
    .unwrap();

    let crate_b = temp.path().join("crate-b");
    fs::create_dir(&crate_b).unwrap();
    fs::write(
        crate_b.join("Cargo.toml"),
        r#"
[package]
name = "crate-b"
version = "0.1.0"
edition = "2021"

[dependencies]
crate-a = { path = "../crate-a" }
"#,
    )
    .unwrap();

    fs::create_dir(crate_b.join("src")).unwrap();
    fs::write(
        crate_b.join("src/lib.rs"),
        "use crate_a;\npub fn greet() {}",
    )
    .unwrap();

    temp
}

#[allow(dead_code)]
fn verify_workspace_valid(workspace_root: &Path) -> bool {
    std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .current_dir(workspace_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_invalid_directory_names() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Test path traversal attempt
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("evil-crate")
        .arg("--move")
        .arg("../../../etc/passwd")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cannot navigate outside workspace",
        ));

    // Test absolute path (Unix-style)
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("evil-crate")
        .arg("--move")
        .arg("/tmp/evil")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("must be a relative path"));

    // Test dot and double-dot
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-crate")
        .arg("--move")
        .arg(".")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot use '.' or '..'"));

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-crate")
        .arg("--move")
        .arg("..")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot use '.' or '..'"));

    // Test parent directory navigation
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-crate")
        .arg("--move")
        .arg("../sibling")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cannot navigate outside workspace",
        ));

    // Windows-specific tests
    #[cfg(windows)]
    {
        // Test Windows absolute path with drive letter
        let mut cmd = cargo_bin_cmd!("cargo-rename");
        cmd.arg("rename")
            .arg("crate-a")
            .arg("evil-crate")
            .arg("--move")
            .arg("C:\\evil")
            .arg("--yes")
            .current_dir(workspace_root)
            .assert()
            .failure()
            .stderr(predicate::str::contains("must be a relative path"));

        // Test Windows reserved name
        let mut cmd = cargo_bin_cmd!("cargo-rename");
        cmd.arg("rename")
            .arg("crate-a")
            .arg("new-crate")
            .arg("--move")
            .arg("CON")
            .arg("--yes")
            .current_dir(workspace_root)
            .assert()
            .failure()
            .stderr(predicate::str::contains("reserved name"));

        // Test invalid Windows characters
        let mut cmd = cargo_bin_cmd!("cargo-rename");
        cmd.arg("rename")
            .arg("crate-a")
            .arg("new-crate")
            .arg("--move")
            .arg("dir:name")
            .arg("--yes")
            .current_dir(workspace_root)
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot contain"));
    }
}

#[test]
fn test_target_directory_already_exists() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Create conflicting directory
    fs::create_dir(workspace_root.join("existing-dir")).unwrap();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-crate")
        .arg("--move")
        .arg("existing-dir")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_nested_directory_creation() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Parent directory doesn't exist yet - should be created
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-crate")
        .arg("--move")
        .arg("crates/backend/api")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    assert!(
        workspace_root
            .join("crates/backend/api/Cargo.toml")
            .exists()
    );

    let cargo_toml =
        fs::read_to_string(workspace_root.join("crates/backend/api/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"new-crate\""));
}

#[test]
fn test_same_name_without_move_fails() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("crate-a")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("same as the old name"));
}

#[test]
fn test_same_name_with_move_succeeds() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // This should work: moving to new directory with same package name
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("crate-a")
        .arg("--move")
        .arg("new-location")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    let cargo_toml = fs::read_to_string(workspace_root.join("new-location/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"crate-a\""));

    // Old directory should be gone
    assert!(!workspace_root.join("crate-a").exists());
}

#[test]
fn test_move_without_custom_path() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // --move without argument should move to directory matching package name
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("awesome-crate")
        .arg("--move")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    // Should be in ./awesome-crate/ directory
    let cargo_toml = fs::read_to_string(workspace_root.join("awesome-crate/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"awesome-crate\""));

    assert!(!workspace_root.join("crate-a").exists());
    assert!(workspace_root.join("awesome-crate").exists());
}

#[test]
fn test_move_with_custom_path() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // --move with custom path
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("awesome-crate")
        .arg("--move")
        .arg("custom-dir")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    // Package renamed to awesome-crate but directory is custom-dir
    let cargo_toml = fs::read_to_string(workspace_root.join("custom-dir/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"awesome-crate\""));

    assert!(!workspace_root.join("crate-a").exists());
    assert!(workspace_root.join("custom-dir").exists());
    assert!(!workspace_root.join("awesome-crate").exists());
}

#[test]
fn test_workspace_dependencies_updated() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    // Create workspace with workspace.dependencies
    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/crate-a", "crates/crate-c"]

[workspace.dependencies]
crate-a = { path = "crates/crate-a" }
crate-c = { path = "crates/crate-c" }
"#,
    )
    .unwrap();

    let crates_dir = workspace_root.join("crates");
    fs::create_dir(&crates_dir).unwrap();

    let crate_a = crates_dir.join("crate-a");
    fs::create_dir(&crate_a).unwrap();
    fs::write(
        crate_a.join("Cargo.toml"),
        r#"[package]
name = "crate-a"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(crate_a.join("src")).unwrap();
    fs::write(crate_a.join("src/lib.rs"), "").unwrap();

    let crate_c = crates_dir.join("crate-c");
    fs::create_dir(&crate_c).unwrap();
    fs::write(
        crate_c.join("Cargo.toml"),
        r#"[package]
name = "crate-c"
version = "0.1.0"
edition = "2021"

[dependencies]
crate-a = { workspace = true }
"#,
    )
    .unwrap();
    fs::create_dir(crate_c.join("src")).unwrap();
    fs::write(crate_c.join("src/lib.rs"), "").unwrap();

    // Run rename
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("crate-b")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    // Verify workspace.dependencies was updated
    let workspace_toml = fs::read_to_string(workspace_root.join("Cargo.toml")).unwrap();
    assert!(
        workspace_toml.contains("crate-b"),
        "Expected crate-b in workspace.dependencies: {}",
        workspace_toml
    );
    assert!(
        !workspace_toml.contains("crate-a = {"),
        "Old crate-a should be removed from workspace.dependencies: {}",
        workspace_toml
    );
}
