mod cli;
mod ops;

use clap::Parser;
use cli::{CargoCli, CargoCommand};
use colored::*;

fn main() {
    let CargoCli { command } = CargoCli::parse();

    match command {
        CargoCommand::Rename(args) => {
            if let Err(e) = ops::execute_rename(args) {
                eprintln!("{}: {:?}", "Error".red().bold(), e);
                std::process::exit(1);
            }
        }
    }
}
