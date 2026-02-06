//! User confirmation prompt for rename operations.
//!
//! Displays a plan and waits for confirmation. Skipped if `--yes` or `--dry-run`.

use crate::error::Result;
use crate::steps::rename::RenameArgs;
use cargo_metadata::Metadata;
use colored::Colorize;
use std::io::{self, Write};

/// Prompts user for confirmation before executing rename.
///
/// ## Automatic Skip
/// - `--yes` or `--dry-run` flag set
/// - Non-interactive terminal (Unix only)
///
/// Returns `true` if confirmed or skipped, `false` if declined.
pub fn confirm_operation(args: &RenameArgs, metadata: &Metadata) -> Result<bool> {
    if args.yes || args.dry_run {
        return Ok(true);
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        if unsafe { libc::isatty(std::io::stdin().as_raw_fd()) == 0 } {
            log::warn!("Non-interactive terminal detected. Use --yes to confirm automatically.");
            return Ok(false);
        }
    }

    let pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .unwrap();

    let dependents: Vec<_> = metadata
        .packages
        .iter()
        .filter(|p| {
            p.dependencies
                .iter()
                .any(|d| d.name == args.old_name || d.rename.as_deref() == Some(&args.old_name))
        })
        .collect();

    println!("\n{}", "Rename Plan:".bold().cyan());
    println!(
        "  {} {} → {}",
        "Package:".bold(),
        args.old_name.yellow(),
        args.new_name.green()
    );

    println!("  {} Update package name in Cargo.toml", "✓".green());
    println!("  {} Update source code references", "✓".green());
    println!("  {} Update workspace dependencies", "✓".green());

    if args.should_move() {
        let old_dir = pkg.manifest_path.parent().unwrap().as_std_path();
        let new_dir = args
            .calculate_new_dir(old_dir, metadata.workspace_root.as_std_path())
            .unwrap();
        let old_dir_name = old_dir.file_name().unwrap().to_string_lossy();
        let new_dir_relative = new_dir
            .strip_prefix(metadata.workspace_root.as_std_path())
            .unwrap_or(&new_dir);

        println!(
            "  {} Move directory: {} → {}",
            "✓".green(),
            old_dir_name.yellow(),
            new_dir_relative.display().to_string().green()
        );
        println!("  {} Update workspace members list", "✓".green());
    }

    if !dependents.is_empty() {
        println!(
            "  {} Update {} dependent package{}",
            "✓".green(),
            dependents.len(),
            if dependents.len() == 1 { "" } else { "s" }
        );
        for (idx, dep) in dependents.iter().enumerate() {
            if idx < 5 {
                println!("    • {}", dep.name);
            }
        }
        if dependents.len() > 5 {
            println!("    • ... and {} more", dependents.len() - 5);
        }
    }

    println!();

    print!("{} {} ", "Continue?".bold(), "(y/N)".dimmed());
    io::stdout().flush()?;

    let mut response = String::new();
    io::stdin().read_line(&mut response)?;

    let confirmed =
        response.trim().eq_ignore_ascii_case("y") || response.trim().eq_ignore_ascii_case("yes");

    if !confirmed {
        log::info!("Rename cancelled by user");
    }

    Ok(confirmed)
}
