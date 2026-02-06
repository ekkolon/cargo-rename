//! Command-line interface definition.
//!
//! Defines the CLI structure using `clap`. Actual rename logic is in `steps/rename.rs`.

use clap::{ColorChoice, Parser, Subcommand};

/// Top-level cargo subcommand.
///
/// Usage: `cargo rename old-name new-name`
#[derive(Parser)]
#[command(
    name = "cargo-rename",
    bin_name = "cargo",
    version,
    propagate_version = true
)]
#[command(styles = clap_cargo::style::CLAP_STYLING)]
pub struct CargoCli {
    #[command(subcommand)]
    pub command: CargoCommand,

    /// Control color output
    #[arg(
        long,
        value_name = "WHEN",
        default_value = "auto",
        global = true,
        display_order = 100
    )]
    pub color: ColorChoice,

    /// Decrease logging verbosity
    #[arg(
        long,
        short = 'q',
        action = clap::ArgAction::Count,
        global = true,
        conflicts_with = "verbose",
        display_order = 101
    )]
    pub quiet: u8,

    /// Increase logging verbosity (-v, -vv, -vvv)
    #[arg(
        long,
        short = 'v',
        action = clap::ArgAction::Count,
        global = true,
        display_order = 102
    )]
    pub verbose: u8,
}

#[derive(Subcommand)]
pub enum CargoCommand {
    /// Perform a coordinated, all-or-nothing rename of a Cargo package
    ///
    /// This command performs a transactional rename and automatically updates:
    ///   • Package name in Cargo.toml
    ///   • All workspace dependency declarations (including workspace.dependencies)
    ///   • Rust source code references (use paths, qualified paths, doc links)
    ///   • Workspace member paths (if --move)
    ///   • Package directory (if --move)
    ///
    /// If any step fails, all changes are rolled back automatically.
    ///
    /// By default, only the package name is renamed. Use --move to relocate the directory.
    /// No files are modified until you confirm the operation."
    #[clap(verbatim_doc_comment)]
    Rename(crate::steps::rename::RenameArgs),
}
