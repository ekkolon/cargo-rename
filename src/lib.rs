#![doc = include_str!("../README.md")]

//! A tool for renaming Cargo packages, handling all references automatically.
//!
//! ## Usage
//!
//! Use the main entry point:
//! ```no_run
//! # fn main() -> cargo_rename::Result<()> {
//! cargo_rename::run()?;
//! # Ok(())
//! # }
//! ```
//!
//! Or directly control the rename:
//! ```no_run
//! # fn example() -> cargo_rename::Result<()> {
//! let args = cargo_rename::RenameArgs {
//!     old_name: "old-crate".into(),
//!     new_name: "new-crate".into(),
//!     outdir: None,
//!     manifest_path: None,
//!     dry_run: false,
//!     yes: true,
//!     allow_dirty: false,
//! };
//!
//! cargo_rename::execute(args)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Modules
//!
//! - **`cli`**: Command-line argument definitions
//! - **`steps`**: Orchestration logic
//! - **`error`**: Error types
//! - **`fs`**: Transaction support
//! - **`cargo`**: Manifest manipulation
//! - **`rewrite`**: Source code rewriting
//! - **`verify`**: Validation checks

pub mod cli;
pub mod error;
pub mod steps;

// Internal modules
pub mod cargo;
pub mod fs;
pub mod rewrite;
pub mod verify;

pub use error::{RenameError, Result};
pub use steps::rename::{RenameArgs, execute};

use clap::Parser;
use log::LevelFilter;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Main entry point.
///
/// Parses CLI args, sets up logging, and executes the rename.
pub fn run() -> Result<()> {
    let cargo_args = cli::CargoCli::parse();

    setup_logging(cargo_args.verbose, cargo_args.quiet);
    setup_colors(cargo_args.color);

    match cargo_args.command {
        cli::CargoCommand::Rename(args) => steps::rename::execute(args),
    }
}

/// Configures logging verbosity.
///
/// Levels: `-v` (warn), `-vv` (info), `-vvv` (debug), `-vvvv` (trace), `-q` (off).
fn setup_logging(verbose: u8, quiet: u8) {
    let log_level = if quiet > 0 {
        LevelFilter::Off
    } else {
        match verbose {
            0 => LevelFilter::Error,
            1 => LevelFilter::Warn,
            2 => LevelFilter::Info,
            3 => LevelFilter::Debug,
            _ => LevelFilter::Trace,
        }
    };

    env_logger::Builder::new()
        .filter_level(log_level)
        .format_timestamp(None)
        .init();
}

/// Configures colored output.
fn setup_colors(choice: clap::ColorChoice) {
    use colored::control;

    match choice {
        clap::ColorChoice::Always => control::set_override(true),
        clap::ColorChoice::Never => control::set_override(false),
        clap::ColorChoice::Auto => {} // colored crate handles automatically
    }
}
