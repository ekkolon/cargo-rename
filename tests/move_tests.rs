mod common;

use std::fs;

use common::*;
use tempfile::TempDir;

#[test]
fn test_move_without_argument_renames_in_place() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // --move without argument should rename directory in place
    run_rename(workspace_root, "crate-a", "awesome-crate", &["--move"]).success();

    // Should be in ./awesome-crate/ (same level as crate-a was)
    assert!(workspace_root.join("awesome-crate/Cargo.toml").exists());
    assert!(!workspace_root.join("crate-a").exists());

    let cargo_toml = fs::read_to_string(workspace_root.join("awesome-crate/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"awesome-crate\""));

    // Verify workspace.members was updated
    let workspace_toml = fs::read_to_string(workspace_root.join("Cargo.toml")).unwrap();
    assert!(workspace_toml.contains("\"awesome-crate\""));
    assert!(!workspace_toml.contains("\"crate-a\""));
}

#[test]
fn test_move_with_custom_path() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    run_rename(
        workspace_root,
        "crate-a",
        "awesome-crate",
        &["--move", "custom-dir"],
    )
    .success();

    // Package renamed to awesome-crate but directory is custom-dir
    assert!(workspace_root.join("custom-dir/Cargo.toml").exists());
    assert!(!workspace_root.join("crate-a").exists());
    assert!(!workspace_root.join("awesome-crate").exists());

    let cargo_toml = fs::read_to_string(workspace_root.join("custom-dir/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"awesome-crate\""));
}

#[test]
fn test_move_to_nested_directory() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Parent directories should be created automatically
    run_rename(
        workspace_root,
        "crate-a",
        "new-crate",
        &["--move", "crates/backend/api"],
    )
    .success();

    assert!(
        workspace_root
            .join("crates/backend/api/Cargo.toml")
            .exists()
    );

    let cargo_toml =
        fs::read_to_string(workspace_root.join("crates/backend/api/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"new-crate\""));

    // Workspace.members should be updated
    let workspace_toml = fs::read_to_string(workspace_root.join("Cargo.toml")).unwrap();
    assert!(workspace_toml.contains("\"crates/backend/api\""));
}

#[test]
fn test_move_same_name_different_location() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Moving to new directory with same package name should work
    run_rename(
        workspace_root,
        "crate-a",
        "crate-a",
        &["--move", "new-location"],
    )
    .success();

    assert!(workspace_root.join("new-location/Cargo.toml").exists());
    assert!(!workspace_root.join("crate-a").exists());

    let cargo_toml = fs::read_to_string(workspace_root.join("new-location/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"crate-a\""));
}

#[test]
fn test_move_updates_dependent_paths() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    // Move crate-a, which crate-b depends on
    run_rename(
        workspace_root,
        "crate-a",
        "new-crate",
        &["--move", "libs/core"],
    )
    .success();

    // Check crate-b's dependency path was updated
    let crate_b_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        crate_b_toml.contains("new-crate") && crate_b_toml.contains("../libs/core"),
        "Expected updated dependency in crate-b:\n{}",
        crate_b_toml
    );

    // Verify workspace is still valid
    assert!(verify_workspace_valid(workspace_root));
}

#[test]
fn test_move_from_nested_to_root() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    // Create workspace with nested package
    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/backend/api"]
"#,
    )
    .unwrap();

    let nested_dir = workspace_root.join("crates/backend/api");
    fs::create_dir_all(&nested_dir).unwrap();
    fs::write(
        nested_dir.join("Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(nested_dir.join("src")).unwrap();
    fs::write(nested_dir.join("src/lib.rs"), "").unwrap();

    // Move up to root level
    run_rename(workspace_root, "api", "api", &["--move", "api"]).success();

    assert!(workspace_root.join("api/Cargo.toml").exists());
    assert!(!nested_dir.exists());

    let workspace_toml = fs::read_to_string(workspace_root.join("Cargo.toml")).unwrap();
    assert!(workspace_toml.contains("\"api\""));
    assert!(!workspace_toml.contains("crates/backend/api"));
}
