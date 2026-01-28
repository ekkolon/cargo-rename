# cargo-rename

[![Crates.io](https://img.shields.io/crates/v/cargo-rename.svg)](https://crates.io/crates/cargo-rename)
[![Documentation](https://docs.rs/cargo-rename/badge.svg)](https://docs.rs/cargo-rename)
[![License](https://img.shields.io/crates/l/cargo-rename.svg)](LICENSE)

Rename Cargo packages safely and consistently across an entire workspace.

`cargo-rename` performs coordinated updates to package manifests, workspace configuration, directory paths, and Rust source code. It is designed for non-trivial renames where manual edits are error-prone and incomplete.

A renaming operation is **transactional**, meaning that if any step fails, all changes are rolled back.

---

## Contents

- [Installation](#installation)
- [Usage](#usage)
- [Command-line options](#command-line-options)
- [Examples](#examples)
- [What gets updated](#what-gets-updated)
- [Safety model](#safety-model)
- [Limitations](#limitations)
- [Development](#development)
- [Contributing](#contributing)
- [License](#license)
- [FAQ](#faq)

---

## Installation

### From crates.io

```bash
cargo install cargo-rename
```

### From GitHub

```bash
cargo install --git https://github.com/ekkolon/cargo-rename
```

## Usage

### Basic operations

```bash
# Rename the package name (default)
cargo rename old-crate new-crate

# Rename the directory only (package name unchanged)
cargo rename old-crate new-dir --path-only

# Rename both package name and directory
cargo rename old-crate new-crate --both

# Preview changes without writing anything
cargo rename old-crate new-crate --dry-run

# Skip confirmation prompt
cargo rename old-crate new-crate --yes
```

### Command-line options

```text
cargo rename [OPTIONS] <OLD_NAME> <NEW_NAME>

Arguments:
  <OLD_NAME>   Current package name
  <NEW_NAME>   New package or directory name

Options:
      --name-only               Rename the package name only (default)
      --path-only               Rename/move the directory only
  -b, --both                    Rename both package and directory
      --manifest-path <PATH>    Path to Cargo.toml
  -n, --dry-run                 Preview changes without applying them
  -y, --yes                     Skip confirmation prompt
      --allow-dirty             Allow uncommitted git changes
  -v, --verbose                 Show detailed progress information
  -h, --help                    Print help
  -V, --version                 Print version
```

## Examples

### Renaming a dependency with an alias

**Before**:

```toml
[dependencies]
my-alias = { package = "old-crate", path = "../old-crate" }
```

```bash
cargo rename old-crate new-crate
```

**After**:

```toml
[dependencies]
my-alias = { package = "new-crate", path = "../old-crate" }
```

### Directory-only rename

```bash
cargo rename my-crate crates/utils --path-only
```

Useful when reorganizing workspace layout without changing the published package name.

### Dry run first

```bash
cargo rename old-crate new-crate --dry-run
```

Review the planned changes, then apply:

```bash
cargo rename old-crate new-crate --yes
```

### Workspace example

**Given a workspace**:

```perl
my-workspace/
├── Cargo.toml
├── crates/
│   ├── my-lib/
│   │   ├── Cargo.toml
│   │   └── src/
│   └── my-app/
│       ├── Cargo.toml
│       └── src/
```

**Running**:

```bash
cargo rename my-lib awesome-lib --both
```

**Will**:

- Rename the package in `crates/my-lib/Cargo.toml`
- Move `crates/my-lib/` => `crates/awesome-lib/`
- Update all workspace dependency entries
- Rewrite Rust source references (use my_lib::…)
- Update the workspace members list

## What gets updated

### Cargo manifests

- `[package].name`
- Dependency entries in:
  - `[dependencies]`
  - `[dev-dependencies]`
  - `[build-dependencies]`
- Path dependencies when directories move
- Aliased dependencies using `package = "..."`
- Workspace `members` list

### Rust source code

- `use old_crate::...`
- `extern crate old_crate::...`
- Fully-qualified paths (`old_crate::module::...`)

## Safety model

### Pre-flight checks

Before making any changes, `cargo-rename` verifies:

- The target package exists
- The new name is valid according to Cargo rules
- The destination directory does not already exist
- The git working tree is clean (unless `--allow-dirty` is used)

### Transactional execution

All filesystem and manifest changes are tracked. If any step fails, the tool attempts to restore the workspace to its original state.

**Conceptually**:

```text
update manifests
update source code
move directory
verify workspace
```

If verification fails, all prior changes are rolled back.

## Limitations

- Binary names (`[[bin]]`) are not modified
- Macro-generated references may not be detected
- Only references inside the same workspace are updated.

## Development

### Build from source

```bash
git clone https://github.com/ekkolon/cargo-rename
cd cargo-rename
cargo build --release
```

### Run tests

```bash
cargo test
RUST_LOG=debug cargo test
```

## Contributing

Contributions are welcome.

- Keep changes focused
- Add tests for behavior changes
- Ensure `cargo fmt` and `cargo clippy` pass
- Prefer correctness and clarity over convenience
- Submit PR

## License

Licensed under either of:

- MIT License
- Apache License, Version 2.0

## FAQ

### Can I undo a rename?

Yes. Use git to revert, or rename back using the tool.

### Does this work outside a workspace?

Yes. It works for both single crates and workspace members.

### Can I rename multiple packages at once?

No. Packages must be renamed individually.
