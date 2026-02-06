//! Binary entry point for the `cargo-rename` command-line tool.
//!
//! This thin wrapper calls into the library's `run()` function and handles
//! process exit codes.

use std::process;

fn main() {
    if let Err(e) = cargo_rename::run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
