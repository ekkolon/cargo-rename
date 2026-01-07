use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cargo-rename", bin_name = "cargo")]
pub struct CargoCli {
    #[command(subcommand)]
    pub command: CargoCommand,
}

#[derive(Subcommand)]
pub enum CargoCommand {
    /// Rename a cargo package and update all references in the workspace.
    Rename(RenameArgs),
}

#[derive(Parser, Debug)]
pub struct RenameArgs {
    /// The current name of the package to rename
    pub old_name: String,

    /// The new name for the package
    pub new_name: String,

    #[arg(
        long,
        short = 'n',
        default_value_t = true,
        overrides_with = "path_only"
    )]
    pub name_only: bool,

    #[arg(
        long,
        short = 'p',
        default_value_t = true,
        overrides_with = "name_only"
    )]
    pub path_only: bool,

    // /// Also rename the directory to match the new name
    // #[arg(long, default_value_t = true)]
    // pub move_dir: bool,
    /// Path to the Cargo.toml of the package (optional, defaults to current dir search)
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

    /// Do not write changes to disk or move files
    #[arg(long)]
    pub dry_run: bool,
}
