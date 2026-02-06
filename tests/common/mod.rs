//! Integration tests for cargo-rename
//!
//! These tests verify end-to-end behavior by creating real Cargo workspaces
//! and executing rename operations through the command-line interface.

use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Helper to create a basic test workspace with two crates
#[allow(unused)]
pub fn create_test_workspace() -> TempDir {
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

/// Verify workspace can be parsed by Cargo
#[allow(unused)]
pub fn verify_workspace_valid(workspace_root: &Path) -> bool {
    std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--no-deps")
        .current_dir(workspace_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Helper to run a rename command
pub fn run_rename(
    workspace_root: &Path,
    old_name: &str,
    new_name: &str,
    extra_args: &[&str],
) -> assert_cmd::assert::Assert {
    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg(old_name)
        .arg(new_name)
        .arg("--yes")
        .arg("--allow-dirty")
        .args(extra_args)
        .current_dir(workspace_root);

    cmd.assert()
}
