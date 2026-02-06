//! Binary entry point for `cargo-rename`.

use std::process;

fn main() {
    if let Err(e) = cargo_rename::run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
