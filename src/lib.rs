#![doc = include_str!("../README.md")]

//! # cargo-rename
//!
//! A tool for renaming packages in Cargo workspaces, handling all references
//! and dependencies automatically.
//!
//! ## Public API
//!
//! This library can be used programmatically. The main entry point is:
//!
//! ```no_run
//! # fn main() -> cargo_rename::Result<()> {
//! cargo_rename::run()?;
//! # Ok(())
//! # }
//! ```
//!
//! For direct control over the rename process:
//!
//! ```no_run
//! # use cargo_rename::steps::rename::RenameArgs;
//! # fn example() -> cargo_rename::Result<()> {
//! let args = RenameArgs {
//!     old_name: "old-crate".into(),
//!     new_name: "new-crate".into(),
//!     r#move: None,
//!     manifest_path: None,
//!     dry_run: false,
//!     yes: true,
//!     allow_dirty: false,
//! };
//!
//! cargo_rename::steps::rename::execute(args)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Modules
//!
//! - **`cli`**: Command-line argument definitions
//! - **`steps`**: Orchestration logic for rename operations
//! - **`error`**: Error types
//! - **`fs`**: File system transaction support
//! - **`cargo`**: Cargo.toml manifest manipulation
//! - **`rewrite`**: Source code rewriting
//! - **`verify`**: Validation and pre-flight checks
//!
//! Most users should use `run()` or `steps::rename::execute()` rather than calling
//! individual modules directly.

pub mod cli;
pub mod error;
pub mod steps;

// Internal modules (may change between minor versions)
pub mod cargo;
pub mod fs;
pub mod rewrite;
pub mod verify;

pub use error::{RenameError, Result};

use clap::Parser;
use log::LevelFilter;

/// Package version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Main entry point for cargo-rename.
///
/// Parses command-line arguments, sets up logging, and dispatches to the
/// appropriate subcommand handler.
///
/// # Errors
///
/// Returns any error encountered during execution. All errors are of type
/// `RenameError` and include context for debugging.
///
/// # Examples
///
/// ```no_run
/// # fn main() -> cargo_rename::Result<()> {
/// cargo_rename::run()?;
/// # Ok(())
/// # }
/// ```
pub fn run() -> Result<()> {
    let cargo_args = cli::CargoCli::parse();

    setup_logging(cargo_args.verbose, cargo_args.quiet);
    setup_colors(cargo_args.color);

    match cargo_args.command {
        cli::CargoCommand::Rename(args) => steps::rename::execute(args),
    }
}

/// Configures logging based on verbosity flags.
///
/// # Levels
///
/// - No flags: Error only
/// - `-v`: Warn
/// - `-vv`: Info
/// - `-vvv`: Debug
/// - `-vvvv`: Trace
/// - `-q`: Off
///
/// # Arguments
///
/// - `verbose`: Count of `-v` flags
/// - `quiet`: Count of `-q` flags (takes precedence)
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

/// Configures colored output based on user preference.
///
/// # Arguments
///
/// - `choice`: ColorChoice from CLI args (auto, always, never)
fn setup_colors(choice: clap::ColorChoice) {
    use colored::control;

    match choice {
        clap::ColorChoice::Always => control::set_override(true),
        clap::ColorChoice::Never => control::set_override(false),
        clap::ColorChoice::Auto => {
            // handled by colored crate does this automatically
        }
    }
}
