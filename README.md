# cargo-rename

[![Crates.io](https://img.shields.io/crates/v/cargo-rename.svg)](https://crates.io/crates/cargo-rename)
[![Documentation](https://docs.rs/cargo-rename/badge.svg)](https://docs.rs/cargo-rename)
[![License](https://img.shields.io/crates/l/cargo-rename.svg)](LICENSE)

`cargo-rename` performs a coordinated, all-or-nothing rename of a Cargo package.

It handles the necessary updates across Cargo.toml, source code, and the file system to ensure the project remains compilable. This includes:

- **Manifests**: Updating `[package].name` and dependency entries in the workspace.
- **Source Code**: Rewriting `use` statements and qualified paths.
- **Filesystem**: Optionally moving the package directory to match the new name.

**Safety**

All changes execute inside a transaction. Every file write and directory move is tracked. If any step fails, the project is automatically restored to its exact previous state.

## Installation

```bash
cargo install cargo-rename
```

## Usage

```bash
# Rename the package name only (directory stays the same)
cargo rename old-crate new-crate

# Move the package directory only (package name unchanged)
cargo rename old-crate --move new-location

# Rename both package name and move directory
cargo rename old-crate new-crate --move new-location

# Move to a different directory with the new package name
cargo rename old-crate new-crate --move

# Move to a nested path
cargo rename old-crate --move libs/core/new-crate

# Preview changes without writing anything
cargo rename old-crate new-crate --dry-run

# Skip confirmation prompt
cargo rename old-crate new-crate --yes

# Allow operation with uncommitted git changes
cargo rename old-crate new-crate --allow-dirty
```

## CLI Reference

```bash
Usage: cargo rename [OPTIONS] <OLD_NAME> [NEW_NAME]

Arguments:
  <OLD_NAME>  Current name of the package
  [NEW_NAME]  New name for the package (optional if only moving)

Options:
      --move [<DIR>]          Move the package to a new directory
      --manifest-path <PATH>  Path to workspace Cargo.toml
  -n, --dry-run               Preview changes without applying them
  -y, --yes                   Skip interactive confirmation
      --allow-dirty           Allow operation with uncommitted git changes
      --color <WHEN>          Control color output [default: auto] [possible values:
                              auto, always, never]
  -q, --quiet...              Decrease logging verbosity
  -v, --verbose...            Increase logging verbosity (-v, -vv, -vvv)
  -h, --help                  Print help (see more with '--help')
  -V, --version               Print version
```

## Library Usage

You can also use `cargo-rename` programmatically.

```rust
use cargo_rename::{execute, RenameArgs};
use std::path::PathBuf;

fn main() -> cargo_rename::Result<()> {
    let args = RenameArgs {
        old_name: "old-crate".into(),
        new_name: Some("new-crate".into()),
        outdir: Some(Some(PathBuf::from("libs/new-crate"))),
        manifest_path: None,
        dry_run: false,
        skip_confirmation: true,
        allow_dirty: false,
    };

    execute(args)?;
    Ok(())
}
```

## Safety Checks

By default, the tool enforces these checks before running:

- `cargo metadata` must resolve successfully.
- The new name must be a valid crate name.
- The git working directory must be clean.

## Scope and Limitations

- **Binaries**: `[[bin]]` targets are not renamed to preserve binary compatibility.
- **Macros**: Identifiers generated dynamically inside macros may not be detected.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
