# cargo-rename

[![Crates.io](https://img.shields.io/crates/v/cargo-rename.svg)](https://crates.io/crates/cargo-rename)
[![Documentation](https://docs.rs/cargo-rename/badge.svg)](https://docs.rs/cargo-rename)
[![License](https://img.shields.io/crates/l/cargo-rename.svg)](LICENSE)

Rename Cargo packages with confidence.

`cargo-rename` performs coordinated, workspace-wide updates to package manifests, directory paths, dependencies, and source code, keeping complex renames safe, consistent, and reversible.

**You may be looking for**:

- [Installation](#installation)
- [Usage](#usage)
- [CLI Options](#command-line-options)
- [What Gets Updated](#what-gets-updated)
- [Safety](#safety-features)
- [Examples](#examples)
- [Limitations](#limitations)
- [Troubleshooting](#troubleshooting)
- [Development](#development)
- [Contributing](#contributing)
- [License](#license)
- [FAQ](#faq)

## Features

- ✅ **Rename package names** in Cargo.toml
- ✅ **Move package directories** to match new names
- ✅ **Update all workspace dependencies** automatically
- ✅ **Update source code references** (use statements, extern crate, etc.)
- ✅ **Handle package aliases** (`package = "..."` syntax)
- ✅ **Support dev-dependencies and build-dependencies**
- ✅ **Dry-run mode** to preview changes
- ✅ **Transaction-based rollback** on errors
- ✅ **Git integration** (optional commit creation)
- ✅ **Workspace verification** after changes

## Installation

### Install from crates.io

```bash
cargo install cargo-rename
```

### Install from GitHub Repository

```bash
cargo install --git https://github.com/ekkolon/cargo-rename
```

## Usage

### Basic Examples

```bash
# Rename package name only (default)
cargo rename old-crate new-crate

# Rename directory only (keeps package name)
cargo rename old-crate new-dir --path-only

# Rename both package name and directory
cargo rename old-crate new-crate --both

# Preview changes without applying them
cargo rename old-crate new-crate --dry-run

# Skip confirmation prompt
cargo rename old-crate new-crate --yes

# Create a git commit after successful rename
cargo rename old-crate new-crate --git-commit
```

### Workspace Example

Given a workspace structure:

```txt
my-workspace/
├── Cargo.toml
├── crates/
│   ├── my-lib/
│   │   ├── Cargo.toml
│   │   └── src/
│   └── my-app/
│       ├── Cargo.toml (depends on my-lib)
│       └── src/
```

**Running**:

```bash
cd my-workspace
cargo rename my-lib awesome-lib --both
```

**Will update**:

- `crates/my-lib/Cargo.toml` → package name to "awesome-lib"
- `crates/my-lib/` → moved to `crates/awesome-lib/`
- `crates/my-app/Cargo.toml` → dependency path updated
- `crates/my-app/src/*.rs` → `use my_lib` → `use awesome_lib`
- Root `Cargo.toml` → workspace members list

## Command-Line Options

```bash
cargo rename [OPTIONS] <OLD_NAME> <NEW_NAME>

Arguments:
  <OLD_NAME>  Current name of the package
  <NEW_NAME>  New name for the package

Options:
      --name-only               Only rename the package name (default)
      --path-only               Only move/rename the directory
  -b, --both                    Rename both name and directory
      --manifest-path <PATH>    Path to Cargo.toml
  -d, --dry-run                 Preview changes without writing
  -y, --yes                     Skip confirmation prompt
      --skip-git-check          Skip checking for uncommitted changes
      --git-commit              Create git commit after rename
  -h, --help                    Print help
  -V, --version                 Print version
```

## What Gets Updated

### Package Manifests

- Package name in `[package]` section
- Dependency references in `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`
- Path dependencies (when directory moves)
- Package aliases (`my-alias = { package = "old-name", ... }`)
- Workspace members list

### Source Code

- `use old_crate::*` → `use new_crate::*`
- `extern crate old_crate` → `extern crate new_crate`
- Module paths: `old_crate::module::*`

### Documentation

- Package name in README.md files
- References in other .md and .toml files

## Safety Features

### Pre-flight Checks

- Validates new package name against Cargo rules
- Checks that target package exists
- Verifies no uncommitted git changes (optional)
- Confirms target directory doesn't exist

### Transaction System

All changes are tracked and can be rolled back atomically if any step fails:

```rust
// Pseudocode
transaction {
    update_manifest_1()  ✓
    update_manifest_2()  ✓
    move_directory()     ✗ ERROR!
    // Automatic rollback of all changes
}
```

### Validation

After successful rename, runs `cargo metadata` to ensure workspace is still valid.

## Examples

### Rename with Package Alias

```toml
# Before
[dependencies]
my-alias = { package = "old-crate", path = "../old-crate" }
```

```bash
cargo rename old-crate new-crate --name-only
```

```toml
# After
[dependencies]
my-alias = { package = "new-crate", path = "../old-crate" }
```

### Directory-Only Rename

```bash
# Move crate to new directory without changing package name
cargo rename my-crate new-directory --path-only
```

Useful for reorganizing workspace structure without breaking published package names.

### Dry Run First

```bash
# Preview all changes
cargo rename old-crate new-crate --dry-run

# If looks good, apply changes
cargo rename old-crate new-crate --yes
```

## Limitations

- **Published crates**: Renaming published crates doesn't update crates.io. Use this for workspace-internal crates or pre-publication.
- **Binary names**: Only updates package name, not `[[bin]]` names.
- **Complex macros**: May not catch all macro-generated references.
- **External workspaces**: Only updates references within the same workspace.

## Troubleshooting

### "Workspace verification failed"

If verification fails after rename:

1. Check `Cargo.toml` syntax with `cargo check`
2. Ensure all paths are correct
3. Run `cargo update` to refresh Cargo.lock
4. Use `--skip-git-check` if git errors occur

### "Package not found"

- Ensure you're running from workspace root
- Use `--manifest-path` to specify exact Cargo.toml location
- Check package name matches exactly (case-sensitive)

### Rollback Didn't Complete

In rare cases, rollback may fail. Manual fixes:

```bash
git reset --hard  # If using git
cargo clean       # Clean build artifacts
```

## Development

### Building from Source

```bash
git clone https://github.com/ekkolon/cargo-rename
cd cargo-rename
cargo build --release
```

### Running Tests

```bash
# Unit and integration tests
cargo test

# With logging
RUST_LOG=debug cargo test

# Specific test
cargo test test_rename_package_name_only
```

### Project Structure

## Contributing

Contributions welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass: `cargo test`
5. Run `cargo fmt` and `cargo clippy`
6. Submit a pull request

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## FAQ

**Q: Can I undo a rename?**  
A: Use git to revert (`git reset --hard`), or rename back using the tool.

**Q: Does this work with non-workspace crates?**  
A: Yes! Works for both standalone crates and workspace members.

**Q: Will this update crates.io?**  
A: No. This tool only updates local files. Publishing a renamed crate creates a new package on crates.io.

**Q: What about binary names?**  
A: Currently only updates package names. Binary names in `[[bin]]` sections are unchanged.

**Q: Can I rename multiple packages at once?**  
A: Not currently supported. Rename packages one at a time.
