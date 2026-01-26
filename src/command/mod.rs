pub mod rename;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum CargoCommand {
    Rename(rename::RenameArgs),
}
