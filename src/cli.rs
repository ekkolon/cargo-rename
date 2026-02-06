//! Command-line interface definition.
//!
//! Defines the CLI structure using `clap`. Actual rename logic is in `steps/rename.rs`.

use clap::{ColorChoice, Parser, Subcommand};

/// Top-level cargo subcommand.
///
/// Usage: `cargo rename old-name new-name`
#[derive(Parser)]
#[command(name = "cargo-rename", bin_name = "cargo", version)]
#[command(styles = clap_cargo::style::CLAP_STYLING)]
pub struct CargoCli {
    #[command(subcommand)]
    pub command: CargoCommand,

    /// Increase logging verbosity (-v, -vv, -vvv)
    #[arg(
        long,
        short = 'v',
        action = clap::ArgAction::Count,
        global = true,
        help_heading = "Global Options"
    )]
    pub verbose: u8,

    /// Decrease logging verbosity
    #[arg(
        long,
        short = 'q',
        action = clap::ArgAction::Count,
        global = true,
        conflicts_with = "verbose",
        help_heading = "Global Options"
    )]
    pub quiet: u8,

    /// Control color output: auto, always, never
    #[arg(
        long,
        value_name = "WHEN",
        default_value = "auto",
        global = true,
        help_heading = "Global Options"
    )]
    pub color: ColorChoice,
}

#[derive(Subcommand)]
pub enum CargoCommand {
    #[clap(
        verbatim_doc_comment,
        about = "Rename a Cargo package and update all workspace references",
        long_about = "Safely rename a Cargo package and update all workspace references.

This command performs a transactional rename and automatically updates:
  • Package name in Cargo.toml
  • All workspace dependency declarations (including workspace.dependencies)
  • Rust source code references (use paths, qualified paths, doc links)
  • Workspace member paths (if --move)
  • Package directory (if --move)

If any step fails, all changes are rolled back automatically.

By default, only the package name is renamed. Use --move to relocate the directory.
No files are modified until you confirm the operation."
    )]
    Rename(crate::steps::rename::RenameArgs),
}
