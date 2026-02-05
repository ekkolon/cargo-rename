use crate::command::CargoCommand;
use clap::{ColorChoice, Parser};

#[derive(Parser)]
#[command(name = "cargo-rename", bin_name = "cargo", version)]
#[command(styles = clap_cargo::style::CLAP_STYLING)]
pub struct CargoCli {
    #[command(subcommand)]
    pub(crate) command: CargoCommand,

    /// Increase logging verbosity
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

    /// Coloring: auto, always, never
    #[arg(
        long,
        value_name = "WHEN",
        default_value = "auto",
        global = true,
        help_heading = "Global Options"
    )]
    pub color: ColorChoice,
}
