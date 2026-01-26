pub mod cli;
pub mod command;
pub mod error;
pub mod ops;
pub mod validation;

pub use error::*;

pub fn run() -> Result<()> {
    use clap::Parser;
    use command::CargoCommand;

    let cli = cli::CargoCli::parse();
    match cli.command {
        CargoCommand::Rename(args) => command::rename::execute(args),
    }
}
