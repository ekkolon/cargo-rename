use crate::command::CargoCommand;
use clap::Parser;

#[derive(Parser)]
#[command(name = "cargo-rename", bin_name = "cargo", version)]
pub struct CargoCli {
    #[command(subcommand)]
    pub(crate) command: CargoCommand,
}
