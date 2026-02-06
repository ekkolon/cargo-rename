mod common;

use std::fs;

use common::*;
use tempfile::TempDir;

#[test]
fn test_rename_with_renamed_dependency() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"[workspace]
members = ["crate-a", "crate-b"]
"#,
    )
    .unwrap();

    let crate_a = workspace_root.join("crate-a");
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

    let crate_b = workspace_root.join("crate-b");
    fs::create_dir(&crate_b).unwrap();
    fs::write(
        crate_b.join("Cargo.toml"),
        r#"[package]
name = "crate-b"
version = "0.1.0"
edition = "2021"

[dependencies]
alias = { package = "crate-a", path = "../crate-a" }
"#,
    )
    .unwrap();
    fs::create_dir(crate_b.join("src")).unwrap();
    fs::write(crate_b.join("src/lib.rs"), "").unwrap();

    run_rename(workspace_root, "crate-a", "new-crate", &[]).success();

    // Alias should be preserved, but package field updated
    let crate_b_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        crate_b_toml.contains("alias = { package = \"new-crate\""),
        "Expected updated package field:\n{}",
        crate_b_toml
    );
}

#[test]
fn test_target_specific_dependencies() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"[workspace]
members = ["crate-a", "crate-b"]
"#,
    )
    .unwrap();

    let crate_a = workspace_root.join("crate-a");
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

    let crate_b = workspace_root.join("crate-b");
    fs::create_dir(&crate_b).unwrap();
    fs::write(
        crate_b.join("Cargo.toml"),
        r#"[package]
name = "crate-b"
version = "0.1.0"
edition = "2021"

[target.'cfg(windows)'.dependencies]
crate-a = { path = "../crate-a" }
"#,
    )
    .unwrap();
    fs::create_dir(crate_b.join("src")).unwrap();
    fs::write(crate_b.join("src/lib.rs"), "").unwrap();

    run_rename(workspace_root, "crate-a", "windows-crate", &[]).success();

    let crate_b_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        crate_b_toml.contains("[target.'cfg(windows)'.dependencies]")
            && crate_b_toml.contains("windows-crate"),
        "Expected target-specific dependency updated:\n{}",
        crate_b_toml
    );
}

#[test]
fn test_inline_table_with_features() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"[workspace]
members = ["crate-a", "crate-b"]
"#,
    )
    .unwrap();

    let crate_a = workspace_root.join("crate-a");
    fs::create_dir(&crate_a).unwrap();
    fs::write(
        crate_a.join("Cargo.toml"),
        r#"[package]
name = "crate-a"
version = "0.1.0"
edition = "2021"

[features]
feat1 = []
feat2 = []
"#,
    )
    .unwrap();
    fs::create_dir(crate_a.join("src")).unwrap();
    fs::write(crate_a.join("src/lib.rs"), "").unwrap();

    let crate_b = workspace_root.join("crate-b");
    fs::create_dir(&crate_b).unwrap();
    fs::write(
        crate_b.join("Cargo.toml"),
        r#"[package]
name = "crate-b"
version = "0.1.0"
edition = "2021"

[dependencies]
crate-a = { path = "../crate-a", features = ["feat1", "feat2"] }
"#,
    )
    .unwrap();
    fs::create_dir(crate_b.join("src")).unwrap();
    fs::write(crate_b.join("src/lib.rs"), "").unwrap();

    run_rename(workspace_root, "crate-a", "new-crate", &[]).success();

    let crate_b_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        crate_b_toml.contains("new-crate") && crate_b_toml.contains("features"),
        "Expected updated dependency with features:\n{}",
        crate_b_toml
    );
}

#[test]
fn test_table_style_dependency() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"[workspace]
members = ["crate-a", "crate-b"]
"#,
    )
    .unwrap();

    let crate_a = workspace_root.join("crate-a");
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

    let crate_b = workspace_root.join("crate-b");
    fs::create_dir(&crate_b).unwrap();
    fs::write(
        crate_b.join("Cargo.toml"),
        r#"[package]
name = "crate-b"
version = "0.1.0"
edition = "2021"

[dependencies.crate-a]
path = "../crate-a"
"#,
    )
    .unwrap();
    fs::create_dir(crate_b.join("src")).unwrap();
    fs::write(crate_b.join("src/lib.rs"), "").unwrap();

    run_rename(workspace_root, "crate-a", "new-crate", &[]).success();

    let crate_b_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        crate_b_toml.contains("[dependencies.new-crate]"),
        "Expected table-style dependency renamed:\n{}",
        crate_b_toml
    );
}

#[test]
fn test_optional_dependency() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"[workspace]
members = ["crate-a", "crate-b"]
"#,
    )
    .unwrap();

    let crate_a = workspace_root.join("crate-a");
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

    let crate_b = workspace_root.join("crate-b");
    fs::create_dir(&crate_b).unwrap();
    fs::write(
        crate_b.join("Cargo.toml"),
        r#"[package]
name = "crate-b"
version = "0.1.0"
edition = "2021"

[dependencies]
crate-a = { path = "../crate-a", optional = true }
"#,
    )
    .unwrap();
    fs::create_dir(crate_b.join("src")).unwrap();
    fs::write(crate_b.join("src/lib.rs"), "").unwrap();

    run_rename(workspace_root, "crate-a", "new-crate", &[]).success();

    let crate_b_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        crate_b_toml.contains("new-crate") && crate_b_toml.contains("optional = true"),
        "Expected optional dependency updated:\n{}",
        crate_b_toml
    );
}
