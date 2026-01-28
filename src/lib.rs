#![doc = include_str!("../README.md")]

pub mod cli;
pub mod command;
pub mod error;
pub mod ops;
pub mod validation;

pub use error::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() -> Result<()> {
    use clap::Parser;
    use command::CargoCommand;

    let cli = cli::CargoCli::parse();
    match cli.command {
        CargoCommand::Rename(args) => command::rename::execute(args),
    }
}
