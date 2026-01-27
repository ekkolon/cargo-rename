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
fn test_rename_package_name_only() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("awesome-crate")
        .arg("--name-only")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    let cargo_toml = fs::read_to_string(workspace_root.join("crate-a/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"awesome-crate\""));

    let dep_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        dep_toml.contains("awesome-crate") && dep_toml.contains("path"),
        "Expected awesome-crate with path in:\n{}",
        dep_toml
    );

    let lib_rs = fs::read_to_string(workspace_root.join("crate-b/src/lib.rs")).unwrap();
    assert!(lib_rs.contains("use awesome_crate"));

    assert!(workspace_root.join("crate-a").exists());
    assert!(!workspace_root.join("awesome-crate").exists());
}

#[test]
fn test_rename_directory_only() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-dir")
        .arg("--path-only")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    let cargo_toml = fs::read_to_string(workspace_root.join("new-dir/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"crate-a\""));

    let dep_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        dep_toml.contains("path") && dep_toml.contains("new-dir"),
        "Expected path to new-dir in:\n{}",
        dep_toml
    );

    assert!(!workspace_root.join("crate-a").exists());
    assert!(workspace_root.join("new-dir").exists());

    let workspace_toml = fs::read_to_string(workspace_root.join("Cargo.toml")).unwrap();
    assert!(workspace_toml.contains("new-dir"));

    assert!(verify_workspace_valid(workspace_root));
}

#[test]
fn test_rename_both() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("super-crate")
        .arg("--both")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    let cargo_toml = fs::read_to_string(workspace_root.join("super-crate/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"super-crate\""));

    let dep_toml = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();
    assert!(
        dep_toml.contains("super-crate") && dep_toml.contains("super-crate"),
        "Expected super-crate in:\n{}",
        dep_toml
    );

    let lib_rs = fs::read_to_string(workspace_root.join("crate-b/src/lib.rs")).unwrap();
    assert!(lib_rs.contains("use super_crate"));

    assert!(!workspace_root.join("crate-a").exists());
    assert!(workspace_root.join("super-crate").exists());

    assert!(verify_workspace_valid(workspace_root));
}

#[test]
fn test_dry_run_makes_no_changes() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let original_cargo = fs::read_to_string(workspace_root.join("crate-a/Cargo.toml")).unwrap();
    let original_dep = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-name")
        .arg("--dry-run")
        .current_dir(workspace_root)
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes will be made"));

    let after_cargo = fs::read_to_string(workspace_root.join("crate-a/Cargo.toml")).unwrap();
    let after_dep = fs::read_to_string(workspace_root.join("crate-b/Cargo.toml")).unwrap();

    assert_eq!(original_cargo, after_cargo);
    assert_eq!(original_dep, after_dep);
    assert!(workspace_root.join("crate-a").exists());
}

#[test]
fn test_dry_run_shows_detailed_output() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    let assert = cmd
        .arg("rename")
        .arg("crate-a")
        .arg("awesome-crate")
        .arg("--dry-run")
        .current_dir(workspace_root)
        .assert()
        .success();

    let output = String::from_utf8_lossy(&assert.get_output().stdout);

    // Verify detailed output is shown
    assert!(output.contains("DRY RUN"));
    assert!(output.contains("Package manifest"));
    assert!(output.contains("Cargo.toml"));
    assert!(output.contains("will be modified")); // e.g: "6 files will be modified"
}

#[test]
fn test_verbose_flag_shows_progress() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("new-name")
        .arg("--yes")
        .arg("--verbose")
        .current_dir(workspace_root)
        .assert()
        .success();
}

#[test]
fn test_invalid_package_name_rejected() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("123invalid")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("must start with"));

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("invalid@name")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid character"));

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("test")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("reserved"));
}

#[test]
fn test_package_not_found() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("nonexistent-crate")
        .arg("new-name")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_target_directory_exists() {
    let temp = create_test_workspace();
    let workspace_root = temp.path();

    fs::create_dir(workspace_root.join("existing-dir")).unwrap();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("crate-a")
        .arg("existing-dir")
        .arg("--path-only")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_rename_with_package_alias() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"
[workspace]
members = ["lib-a", "lib-b"]
resolver = "2"
"#,
    )
    .unwrap();

    let lib_a = workspace_root.join("lib-a");
    fs::create_dir(&lib_a).unwrap();
    fs::write(
        lib_a.join("Cargo.toml"),
        r#"
[package]
name = "lib-a"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(lib_a.join("src")).unwrap();
    fs::write(lib_a.join("src/lib.rs"), "pub fn foo() {}").unwrap();

    let lib_b = workspace_root.join("lib-b");
    fs::create_dir(&lib_b).unwrap();
    fs::write(
        lib_b.join("Cargo.toml"),
        r#"
[package]
name = "lib-b"
version = "0.1.0"
edition = "2021"

[dependencies]
my_alias = { package = "lib-a", path = "../lib-a" }
"#,
    )
    .unwrap();
    fs::create_dir(lib_b.join("src")).unwrap();
    fs::write(lib_b.join("src/lib.rs"), "").unwrap();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("lib-a")
        .arg("lib-awesome")
        .arg("--name-only")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    let dep_toml = fs::read_to_string(workspace_root.join("lib-b/Cargo.toml")).unwrap();
    assert!(
        dep_toml.contains("my_alias") && dep_toml.contains("lib-awesome"),
        "Expected alias preserved with new package name in:\n{}",
        dep_toml
    );
}

#[test]
fn test_rename_updates_source_code_patterns() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"
[package]
name = "my-lib"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    fs::create_dir(workspace_root.join("src")).unwrap();
    fs::write(
        workspace_root.join("src/lib.rs"),
        r#"
use my_lib::module;
pub mod module {}
"#,
    )
    .unwrap();

    fs::write(
        workspace_root.join("README.md"),
        "# my-lib\n\nThis is my-lib.",
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("my-lib")
        .arg("awesome-lib")
        .arg("--name-only")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    let lib_rs = fs::read_to_string(workspace_root.join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("use awesome_lib::module"));

    let readme = fs::read_to_string(workspace_root.join("README.md")).unwrap();
    assert!(readme.contains("awesome-lib"));
}

#[test]
fn test_nested_workspace_structure() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"
[workspace]
members = ["crates/*"]
resolver = "2"
"#,
    )
    .unwrap();

    let crates_dir = workspace_root.join("crates");
    fs::create_dir(&crates_dir).unwrap();

    let my_crate = crates_dir.join("my-crate");
    fs::create_dir(&my_crate).unwrap();
    fs::write(
        my_crate.join("Cargo.toml"),
        r#"
[package]
name = "my-crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(my_crate.join("src")).unwrap();
    fs::write(my_crate.join("src/lib.rs"), "").unwrap();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("my-crate")
        .arg("new-crate")
        .arg("--path-only")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    assert!(crates_dir.join("new-crate").exists());
    assert!(!crates_dir.join("my-crate").exists());

    assert!(verify_workspace_valid(workspace_root));
}

#[test]
fn test_multiple_dependents_updated() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"
[workspace]
members = ["lib", "bin1", "bin2"]
resolver = "2"
"#,
    )
    .unwrap();

    let lib_dir = workspace_root.join("lib");
    fs::create_dir(&lib_dir).unwrap();
    fs::write(
        lib_dir.join("Cargo.toml"),
        r#"
[package]
name = "common-lib"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(lib_dir.join("src")).unwrap();
    fs::write(lib_dir.join("src/lib.rs"), "pub fn shared() {}").unwrap();

    for i in 1..=2 {
        let bin_dir = workspace_root.join(format!("bin{}", i));
        fs::create_dir(&bin_dir).unwrap();
        fs::write(
            bin_dir.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "bin{}"
version = "0.1.0"
edition = "2021"

[dependencies]
common-lib = {{ path = "../lib" }}
"#,
                i
            ),
        )
        .unwrap();
        fs::create_dir(bin_dir.join("src")).unwrap();
        fs::write(bin_dir.join("src/main.rs"), "use common_lib; fn main() {}").unwrap();
    }

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("common-lib")
        .arg("shared-utils")
        .arg("--name-only")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    for i in 1..=2 {
        let dep_toml =
            fs::read_to_string(workspace_root.join(format!("bin{}/Cargo.toml", i))).unwrap();
        assert!(dep_toml.contains("shared-utils"));

        let main_rs =
            fs::read_to_string(workspace_root.join(format!("bin{}/src/main.rs", i))).unwrap();
        assert!(main_rs.contains("use shared_utils"));
    }
}

#[test]
fn test_dev_and_build_dependencies() {
    let temp = TempDir::new().unwrap();
    let workspace_root = temp.path();

    fs::write(
        workspace_root.join("Cargo.toml"),
        r#"
[workspace]
members = ["test-utils", "main"]
resolver = "2"
"#,
    )
    .unwrap();

    let test_utils = workspace_root.join("test-utils");
    fs::create_dir(&test_utils).unwrap();
    fs::write(
        test_utils.join("Cargo.toml"),
        r#"
[package]
name = "test-utils"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(test_utils.join("src")).unwrap();
    fs::write(test_utils.join("src/lib.rs"), "").unwrap();

    let main_dir = workspace_root.join("main");
    fs::create_dir(&main_dir).unwrap();
    fs::write(
        main_dir.join("Cargo.toml"),
        r#"
[package]
name = "main"
version = "0.1.0"
edition = "2021"

[dev-dependencies]
test-utils = { path = "../test-utils" }

[build-dependencies]
test-utils = { path = "../test-utils" }
"#,
    )
    .unwrap();
    fs::create_dir(main_dir.join("src")).unwrap();
    fs::write(main_dir.join("src/lib.rs"), "").unwrap();

    let mut cmd = cargo_bin_cmd!("cargo-rename");
    cmd.arg("rename")
        .arg("test-utils")
        .arg("test-helpers")
        .arg("--both")
        .arg("--yes")
        .current_dir(workspace_root)
        .assert()
        .success();

    let main_toml = fs::read_to_string(workspace_root.join("main/Cargo.toml")).unwrap();

    assert!(main_toml.contains("[dev-dependencies]"));
    assert!(main_toml.contains("[build-dependencies]"));
    assert!(
        main_toml.matches("test-helpers").count() >= 2,
        "Expected test-helpers at least twice in:\n{}",
        main_toml
    );
}

#[cfg(test)]
mod validation_tests {
    use cargo_rename::validation::validate_package_name;

    #[test]
    fn test_valid_package_names() {
        assert!(validate_package_name("my-crate").is_ok());
        assert!(validate_package_name("my_crate").is_ok());
        assert!(validate_package_name("a").is_ok());
    }

    #[test]
    fn test_invalid_package_names() {
        assert!(validate_package_name("123crate").is_err());
        assert!(validate_package_name("my@crate").is_err());
        assert!(validate_package_name("test").is_err());
        assert!(validate_package_name("").is_err());
    }
}
