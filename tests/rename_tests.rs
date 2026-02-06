//! Basic rename operation tests (name only, no directory move)

mod common;

use common::*;

use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_simple_rename() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    run_rename(workspace_root, "crate-a", "awesome-crate", &[]).success();

    // Directory name unchanged, but package name updated
    let cargo_toml = fs::read_to_string(workspace_root.join("crate-a/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"awesome-crate\""));

    // Verify workspace is still valid
    assert!(verify_workspace_valid(workspace_root));
}

#[test]
fn test_rename_updates_dependents() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    run_rename(workspace_root, "crate-a", "new-crate", &[]).success();

    // crate-b should have updated dependency
    let crate_b_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(crate_b_toml.contains("new-crate = { path = \"../crate-a\" }"));
    assert!(!crate_b_toml.contains("crate-a = {"));

    // Source code should be updated
    let crate_b_lib = fs::read_to_string(workspace_root.join("crate-b/src/lib.rs")).unwrap();
    assert!(crate_b_lib.contains("use new_crate;"));
    assert!(!crate_b_lib.contains("use crate_a;"));
}

#[test]
fn test_dry_run_does_not_modify() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-crate")
        .arg("--dry-run")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    // Nothing should have changed
    let cargo_toml = fs::read_to_string(workspace_root.join("crate-a/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"crate-a\""));
    assert!(!cargo_toml.contains("name = \"new-crate\""));
}

#[test]
fn test_rename_with_workspace_dependencies() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/crate-a", "crates/crate-c"]

[workspace.dependencies]
crate-a = { path = "crates/crate-a" }
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

    run_rename(workspace_root, "crate-a", "crate-b", &[]).success();

    // Verify workspace.dependencies was updated
    let workspace_toml = fs::read_to_string(workspace_root.join("Cargo.toml")).unwrap();
    assert!(workspace_toml.contains("crate-b = { path = \"crates/crate-a\" }"));
    assert!(!workspace_toml.contains("crate-a = {"));

    // Dependent should still use workspace = true
    let crate_c_toml =
        fs::read_to_string(workspace_root.join("crates/crate-c/Cargo.toml")).unwrap();
    assert!(crate_c_toml.contains("crate-b = { workspace = true }"));
}
