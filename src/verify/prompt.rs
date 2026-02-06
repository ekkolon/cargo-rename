//! User confirmation prompt for rename operations.
//!
//! Displays a plan of what will be changed and waits for user confirmation.
//! Automatically skipped in non-interactive mode or when `--yes` is specified.

use crate::error::Result;
use crate::steps::rename::RenameArgs;
use cargo_metadata::Metadata;
use colored::Colorize;
use std::io::{self, Write};

/// Prompts the user for confirmation before executing the rename.
///
/// # Automatic Skip Conditions
///
/// - `--yes` flag is set
/// - `--dry-run` flag is set (no confirmation needed)
/// - **Unix only**: stdin is not a TTY (non-interactive terminal)
///
/// # Returns
///
/// - `Ok(true)` if user confirms or prompt is skipped
/// - `Ok(false)` if user declines
///
/// # Errors
///
/// Returns `Err` only on I/O errors reading stdin.
pub fn confirm_operation(args: &RenameArgs, metadata: &Metadata) -> Result<bool> {
    // Skip confirmation if flags are set
    if args.yes || args.dry_run {
        return Ok(true);
    }

    // Check for non-interactive terminal (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // Safety: isatty only reads file descriptor metadata
        if unsafe { libc::isatty(std::io::stdin().as_raw_fd()) == 0 } {
            log::warn!("Non-interactive terminal detected. Use --yes to confirm automatically.");
            return Ok(false);
        }
    }

    // Note: On Windows, non-interactive detection is not implemented.
    // The prompt will hang if stdin is redirected. Users should use --yes.

    let pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .unwrap(); // Safe: validated in preflight_checks

    // Find dependent packages
    let dependents: Vec<_> = metadata
        .packages
        .iter()
        .filter(|p| {
            p.dependencies
                .iter()
                .any(|d| d.name == args.old_name || d.rename.as_deref() == Some(&args.old_name))
        })
        .collect();

    // Display rename plan
    println!("\n{}", "Rename Plan:".bold().cyan());
    println!(
        "  {} {} → {}",
        "Package:".bold(),
        args.old_name.yellow(),
        args.new_name.green()
    );

    // Operations that will be performed
    println!("  {} Update package name in Cargo.toml", "✓".green());
    println!("  {} Update source code references", "✓".green());
    println!("  {} Update workspace dependencies", "✓".green());

    // Directory move details
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

    // List dependent packages
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

    // Prompt for user input
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
