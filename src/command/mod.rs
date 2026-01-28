pub mod rename;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum CargoCommand {
    #[clap(
        verbatim_doc_comment,
        about = "Rename a Cargo package and update all affected workspace references",
        long_about = "\
Safely rename a Cargo package and update all affected workspace references.

This command performs a transactional rename operation and automatically updates:
  • The package name in Cargo.toml
  • All workspace dependency declarations
  • Rust source code references (use paths, module paths)
  • Workspace member paths
  • The package directory (optional)

If any step fails, all changes are rolled back automatically.

By default, only the package name is renamed.
No files are modified until you confirm the operation.

EXAMPLES:
  # Rename package name only (default)
  cargo rename old-crate new-crate

  # Preview all changes without modifying files
  cargo rename old-crate new-crate --dry-run

  # Rename both the package and its directory
  cargo rename old-crate new-crate --both

  # Move the directory only (keep the package name)
  cargo rename old-crate new-dir --path-only

  # Skip confirmation and allow dirty git state
  cargo rename old-crate new-crate -y --allow-dirty
"
    )]
    Rename(rename::RenameArgs),
}
