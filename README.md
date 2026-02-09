# cargo-rename

[![Crates.io](https://img.shields.io/crates/v/cargo-rename.svg)](https://crates.io/crates/cargo-rename)
[![Documentation](https://docs.rs/cargo-rename/badge.svg)](https://docs.rs/cargo-rename)
[![License](https://img.shields.io/crates/l/cargo-rename.svg)](LICENSE)

`cargo-rename` performs an atomic rename of a Cargo package, updating all references throughout your workspace in a single operation.

It updates `[package].name` and all dependency references in manifests across workspace members, and rewrites `use` statements, qualified paths, and crate references in Rust source files. Optionally, it can rename the package directory to match the new name or move it to a different location.

**Atomicity**

All modifications are performed atomically. Each file write and directory move is tracked, and if any operation fails, all changes are rolled back to restore the project to its original state.

**Preconditions**

By default, the following checks must pass before execution:

- `cargo metadata` resolves without errors.
- The new name is a valid Rust crate identifier.
- The git working tree is clean (no uncommitted changes).

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

```txt
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

## Limitations

- **Binaries**: `[[bin]]` targets are not renamed to preserve binary compatibility.
- **Macros**: Identifiers generated dynamically inside macros may not be detected.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
