use crate::command::CargoCommand;
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "cargo-rename",
    bin_name = "cargo",
    version,
    about = "Rename Cargo packages and update all workspace references"
)]
pub struct CargoCli {
    #[command(subcommand)]
    pub(crate) command: CargoCommand,
}
