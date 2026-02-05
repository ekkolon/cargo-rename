#![doc = include_str!("../README.md")]

pub mod cli;
pub mod command;
pub mod error;
pub mod ops;
pub mod validation;

pub use error::*;

use clap::Parser;
use command::CargoCommand;
use log::LevelFilter;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() -> Result<()> {
    let cargo_args = cli::CargoCli::parse();

    setup_logging(cargo_args.verbose);

    match cargo_args.command {
        CargoCommand::Rename(args) => command::rename::execute(args),
    }
}

// Setup logging level based on verbose flag
fn setup_logging(verbosity: u8) {
    let log_level = match verbosity {
        0 => LevelFilter::Off,
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        4 => LevelFilter::Info,
        5 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };
    env_logger::Builder::new().filter_level(log_level).init();
    log::set_max_level(log_level);
}
