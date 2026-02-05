pub mod rename;

use clap::Subcommand;

use crate::command::rename::RenameArgs;

#[derive(Subcommand)]
pub enum CargoCommand {
    #[clap(
        verbatim_doc_comment,
        about = "Rename a Cargo package and update all affected workspace references",
        long_about = "Safely rename a Cargo package and update all affected workspace references.

This command performs a transactional rename operation and automatically updates:
  • The package name in Cargo.toml
  • All workspace dependency declarations (including workspace.dependencies)
  • Rust source code references (use paths, module paths)
  • Workspace member paths (if --move is used)
  • The package directory (if --move is used)

If any step fails, all changes are rolled back automatically.

By default, only the package name is renamed. Directory operations require --move.
No files are modified until you confirm the operation."
    )]
    Rename(RenameArgs),
}

const _EXAMPLES: &str = r#"EXAMPLES:
  === Rename package name only (safest - no directory changes)

      cargo rename old-crate new-crate

  === Preview all changes without modifying files

      cargo rename old-crate new-crate --dry-run

  === Rename package AND move directory to match new name
   
      cargo rename old-crate new-crate --move

  === Rename package AND move to custom directory location
   
      cargo rename old-crate new-crate --move custom-location
  
  === Rename package AND move to nested path
   
      cargo rename api-crate api-v2 --move crates/backend/api-v2
  
  === Skip confirmation and allow dirty git state
   
      cargo rename old-crate new-crate --move -y --allow-dirty"#;
