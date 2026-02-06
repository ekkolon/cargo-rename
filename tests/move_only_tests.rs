//! Integration tests for move-only operations (no renaming).

mod common;

use common::*;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn setup_test_workspace(temp: &TempDir) {
    let workspace_toml = temp.path().join("Cargo.toml");
    fs::write(
        &workspace_toml,
        r#"[workspace]
members = ["crates/crate-a", "crates/crate-b"]
resolver = "2"

[workspace.dependencies]
crate-a = { path = "crates/crate-a" }
"#,
    )
    .unwrap();

    // crate-a
    let crate_a_dir = temp.path().join("crates/crate-a");
    fs::create_dir_all(&crate_a_dir).unwrap();
    fs::write(
        crate_a_dir.join("Cargo.toml"),
        r#"[package]
name = "crate-a"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(crate_a_dir.join("src")).unwrap();
    fs::write(crate_a_dir.join("src/lib.rs"), "pub fn hello() {}").unwrap();

    // crate-b (depends on crate-a)
    let crate_b_dir = temp.path().join("crates/crate-b");
    fs::create_dir_all(&crate_b_dir).unwrap();
    fs::write(
        crate_b_dir.join("Cargo.toml"),
        r#"[package]
name = "crate-b"
version = "0.1.0"
edition = "2021"

[dependencies]
crate-a = { path = "../crate-a" }
"#,
    )
    .unwrap();
    fs::create_dir(crate_b_dir.join("src")).unwrap();
    fs::write(
        crate_b_dir.join("src/lib.rs"),
        "use crate_a;\n\npub fn test() { crate_a::hello(); }",
    )
    .unwrap();
}

#[test]
fn test_move_only_to_custom_location() {
    let temp = TempDir::new().unwrap();
    setup_test_workspace(&temp);

    run_rename(temp.path(), "crate-a", "", &["--move", "libs/core"]).success();

    // Check directory moved
    assert!(temp.path().join("libs/core").exists());
    assert!(!temp.path().join("crates/crate-a").exists());

    // Check package name unchanged
    let manifest = fs::read_to_string(temp.path().join("libs/core/Cargo.toml")).unwrap();
    assert!(manifest.contains("name = \"crate-a\""));

    // Check workspace updated
    let workspace_toml = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert!(workspace_toml.contains("libs/core"));
    assert!(!workspace_toml.contains("crates/crate-a"));

    // Check dependent updated
    let crate_b_toml = fs::read_to_string(temp.path().join("crates/crate-b/Cargo.toml")).unwrap();
    assert!(crate_b_toml.contains("path = \"../../libs/core\""));
}

#[test]
fn test_move_without_arg_requires_explicit_dir() {
    let temp = TempDir::new().unwrap();
    setup_test_workspace(&temp);

    run_rename(temp.path(), "crate-a", "", &["--move"])
        .failure()
        .stderr(predicate::str::contains(
            "--move requires an explicit directory",
        ));
}

#[test]
fn test_no_args_fails() {
    let temp = TempDir::new().unwrap();
    setup_test_workspace(&temp);

    run_rename(temp.path(), "crate-a", "", &[])
        .failure()
        .stderr(predicate::str::contains(
            "Must specify either NEW_NAME or --move DIR",
        ));
}

#[test]
fn test_move_to_same_location_no_op() {
    let temp = TempDir::new().unwrap();
    setup_test_workspace(&temp);

    run_rename(temp.path(), "crate-a", "", &["--move", "crates/crate-a"])
        .success()
        .stdout(predicate::str::contains("No changes needed"));
}

#[test]
fn test_rename_and_move_together() {
    let temp = TempDir::new().unwrap();
    setup_test_workspace(&temp);

    run_rename(
        temp.path(),
        "crate-a",
        "awesome-crate",
        &["--move", "libs/awesome"],
    )
    .success();

    // Check both name and location changed
    let manifest = fs::read_to_string(temp.path().join("libs/awesome/Cargo.toml")).unwrap();
    assert!(manifest.contains("name = \"awesome-crate\""));

    // Check source code updated
    let lib_rs = fs::read_to_string(temp.path().join("crates/crate-b/src/lib.rs")).unwrap();
    assert!(lib_rs.contains("use awesome_crate;"));
    assert!(lib_rs.contains("awesome_crate::hello()"));
}

#[test]
fn test_move_creates_parent_directories() {
    let temp = TempDir::new().unwrap();
    setup_test_workspace(&temp);

    run_rename(temp.path(), "crate-a", "", &["--move", "deep/nested/path"]).success();

    assert!(temp.path().join("deep/nested/path").exists());
    assert!(temp.path().join("deep/nested/path/Cargo.toml").exists());
}
