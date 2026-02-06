//! `cargo-rename` performs a coordinated, all-or-nothing rename of a Cargo
//! package.
//!
//! It handles the necessary updates across Cargo.toml, source code, and the
//! file system to ensure the project remains compilable. This includes:
//!
//! - **Manifests**: Updating `[package].name` and dependency entries in the workspace.
//! - **Source Code**: Rewriting `use` statements and qualified paths.
//! - **Filesystem**: Optionally moving the package directory to match the new name.
//!
//! **Safety**
//!
//! All changes execute inside a transaction. Every file write and directory move is
//! tracked. If any step fails, the project is automatically restored to its exact
//! previous state
//!
//! ## Quick Start
//!
//! ```bash
//! cargo install cargo-rename
//! ```
//!
//! ```bash
//! # Rename a package and update all references
//! cargo rename old-name new-name
//!
//! # Rename and move the directory
//! cargo rename old-name new-name --move
//!
//! # Rename and move the directory to a new location
//! cargo rename old-name new-name --move crates/new-name
//!
//! # Dry run to preview changes
//! cargo rename old-name new-name --dry-run
//! ```
//!
//! ## Library Usage
//!
//! You can also use `cargo-rename` programmatically.
//!
//! ```no_run
//! use cargo_rename::{execute, RenameArgs};
//! use std::path::PathBuf;
//!
//! # fn main() -> cargo_rename::Result<()> {
//! let args = RenameArgs {
//!     old_name: "old-crate".into(),
//!     new_name: "new-crate".into(),
//!     outdir: Some(Some(PathBuf::from("libs/new-crate"))),
//!     manifest_path: None,
//!     dry_run: false,
//!     yes: true,
//!     allow_dirty: false,
//! };
//!
//! execute(args)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Scope and Limitations
//!
//! - **Binaries**: `[[bin]]` targets are not renamed to preserve binary compatibility.
//! - **Macros**: Identifiers generated dynamically inside macros may not be detected.
//!
//! ## Safety Checks
//!
//! By default, the tool enforces these checks before running:
//! - `cargo metadata` must resolve successfully.
//! - The new name must be a valid crate name.
//! - The git working directory must be clean.

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
