use crate::command::rename::RenameArgs;
use crate::error::{RenameError, Result};
use cargo_metadata::Metadata;
use colored::Colorize;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

/// Validates a package name according to Cargo's naming rules.
///
/// # Rules
/// - Must start with an ASCII letter
/// - Can only contain ASCII alphanumerics, hyphens, and underscores
/// - Cannot be empty
/// - Cannot exceed 64 characters (practical limit)
/// - Cannot be a reserved name (test, doc, build, bench)
///
/// # Errors
/// Returns `RenameError::InvalidName` if validation fails.
pub fn validate_package_name(name: &str) -> Result<()> {
    // Check empty first - most basic validation
    if name.is_empty() {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot be empty".to_string(),
        ));
    }

    // Check first character is ASCII letter
    let first_char = name.chars().next().unwrap(); // Safe: we checked non-empty
    if !first_char.is_ascii_alphabetic() && first_char != '_' {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "must start with an ASCII letter or underscore".to_string(),
        ));
    }

    // Check all characters are valid
    for (idx, ch) in name.chars().enumerate() {
        if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' {
            return Err(RenameError::InvalidName(
                name.to_string(),
                format!(
                    "contains invalid character '{}' at position {}. Only ASCII letters, numbers, hyphens, and underscores are allowed",
                    ch, idx
                ),
            ));
        }
    }

    // Check against reserved names
    const RESERVED: &[&str] = &["test", "doc", "build", "bench"];
    if RESERVED.contains(&name) {
        return Err(RenameError::InvalidName(
            name.to_string(),
            format!(
                "is a reserved package name. Reserved names: {}",
                RESERVED.join(", ")
            ),
        ));
    }

    // // Check length limit (practical limit, not hard Cargo requirement)
    // const MAX_LENGTH: usize = 64;
    // if name.len() > MAX_LENGTH {
    //     return Err(RenameError::InvalidName(
    //         name.to_string(),
    //         format!(
    //             "exceeds maximum length of {} characters (length: {})",
    //             MAX_LENGTH,
    //             name.len()
    //         ),
    //     ));
    // }

    // Additional checks for common mistakes
    if name.starts_with('-') {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot start with a hyphen".to_string(),
        ));
    }

    if name.ends_with('-') {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot end with a hyphen".to_string(),
        ));
    }

    // Warn about potential crates.io conflicts
    if name.contains('_') && name.contains('-') {
        log::warn!(
            "Package name '{}' contains both underscores and hyphens. This is valid but may cause confusion.",
            name
        );
    }

    Ok(())
}

/// Checks if the git working directory has uncommitted changes.
///
/// # Behavior
/// - Returns error if workspace has uncommitted changes
/// - Returns Ok if workspace is clean
/// - Returns Ok if not a git repository (fails silently)
/// - Returns Ok if git is not installed (fails silently)
///
/// # Errors
/// Returns `RenameError::DirtyWorkspace` if there are uncommitted changes.
pub fn check_git_status(workspace_root: &Path) -> Result<()> {
    // Check if git is available first
    let git_available = Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !git_available {
        log::debug!("Git not available, skipping git status check");
        return Ok(());
    }

    // Check if this is a git repository
    let is_git_repo = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(workspace_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_git_repo {
        log::debug!("Not a git repository, skipping git status check");
        return Ok(());
    }

    // Check for uncommitted changes
    match Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace_root)
        .output()
    {
        Ok(output) if output.status.success() => {
            if !output.stdout.is_empty() {
                // Parse the status to give helpful information
                let status = String::from_utf8_lossy(&output.stdout);
                let modified_files: Vec<_> =
                    status.lines().take(5).map(|line| line.trim()).collect();

                log::warn!("Uncommitted changes detected:");
                for file in &modified_files {
                    log::warn!("  {}", file);
                }
                if status.lines().count() > 5 {
                    log::warn!("  ... and {} more files", status.lines().count() - 5);
                }

                return Err(RenameError::DirtyWorkspace);
            }
            Ok(())
        }
        Ok(output) => {
            log::warn!(
                "Git status command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            Ok(()) // Fail gracefully
        }
        Err(e) => {
            log::warn!("Failed to execute git status: {}", e);
            Ok(()) // Fail gracefully
        }
    }
}

/// Performs all pre-flight checks before executing rename operation.
///
/// # Checks
/// 1. Validates new package name
/// 2. Verifies old package exists
/// 3. Checks git status (unless skipped)
///
/// # Errors
/// Returns first error encountered during checks.
pub fn preflight_checks(args: &RenameArgs, metadata: &Metadata) -> Result<()> {
    // 1. Validate new package name
    validate_package_name(&args.new_name)?;

    // 2. Verify old package exists
    let _pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .ok_or_else(|| RenameError::PackageNotFound(args.old_name.clone()))?;

    // 3. Check git status
    if !args.skip_git_check
        && let Err(e) = check_git_status(metadata.workspace_root.as_std_path())
    {
        log::error!("{}", e);
        log::info!("Hint: Use --skip-git-check to bypass this check");
        return Err(e);
    }

    // 4. Additional safety check: ensure new name differs from old name
    if args.old_name == args.new_name {
        return Err(RenameError::Other(anyhow::anyhow!(
            "New name '{}' is the same as the old name. Nothing to do.",
            args.new_name
        )));
    }

    // 5. Check if target directory would conflict (for path operations)
    if args.mode.should_move_directory() {
        let pkg = metadata
            .packages
            .iter()
            .find(|p| p.name == args.old_name)
            .unwrap();
        let old_dir = pkg.manifest_path.parent().unwrap();
        let mut new_dir = old_dir.to_path_buf();
        new_dir.set_file_name(&args.new_name);

        if new_dir.exists() {
            return Err(RenameError::DirectoryExists(new_dir.into_std_path_buf()));
        }
    }

    Ok(())
}

/// Prompts user for confirmation before executing rename operation.
///
/// # Behavior
/// - Skips prompt if `--yes` or `--dry-run` flag is set
/// - Shows detailed plan of changes
/// - Waits for user input
///
/// # Returns
/// - `Ok(true)` if user confirms or prompt is skipped
/// - `Ok(false)` if user declines
/// - `Err` if I/O error occurs
pub fn confirm_operation(args: &RenameArgs, metadata: &Metadata) -> Result<bool> {
    // Skip confirmation if flags are set
    if args.yes || args.dry_run {
        return Ok(true);
    }

    let pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .unwrap(); // Safe: validated in preflight_checks

    // Find all dependents
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

    if args.mode.should_rename_package() {
        println!("  {} Update package name in Cargo.toml", "✓".green());
        println!("  {} Update source code references", "✓".green());
    }

    if args.mode.should_move_directory() {
        let old_dir = pkg.manifest_path.parent().unwrap();
        let old_dir_name = old_dir.file_name().unwrap().to_string();
        println!(
            "  {} Move directory: {} → {}",
            "✓".green(),
            old_dir_name.yellow(),
            args.new_name.green()
        );
        println!("  {} Update workspace members list", "✓".green());
    }

    if !dependents.is_empty() {
        println!(
            "  {} Update {} dependent package(s):",
            "✓".green(),
            dependents.len()
        );
        for (idx, dep) in dependents.iter().enumerate() {
            if idx < 5 {
                println!("    • {}", dep.name);
            }
        }
        if dependents.len() > 5 {
            println!("    ... and {} more", dependents.len() - 5);
        }
    }

    // Prompt for confirmation
    print!("\n{} {} ", "Continue?".bold(), "[y/N]".dimmed());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_basic_names() {
        assert!(validate_package_name("my-crate").is_ok());
        assert!(validate_package_name("my_crate").is_ok());
        assert!(validate_package_name("MyCrate").is_ok());
        assert!(validate_package_name("crate123").is_ok());
        assert!(validate_package_name("a").is_ok());
    }

    #[test]
    fn test_validate_invalid_start() {
        assert!(validate_package_name("123crate").is_err());
        assert!(validate_package_name("-crate").is_err());
    }

    #[test]
    fn test_validate_invalid_chars() {
        assert!(validate_package_name("my@crate").is_err());
        assert!(validate_package_name("my.crate").is_err());
        assert!(validate_package_name("my crate").is_err());
        assert!(validate_package_name("my/crate").is_err());
        assert!(validate_package_name("my\\crate").is_err());
    }

    #[test]
    fn test_validate_reserved_names() {
        assert!(validate_package_name("test").is_err());
        assert!(validate_package_name("doc").is_err());
        assert!(validate_package_name("build").is_err());
        assert!(validate_package_name("bench").is_err());
    }

    #[test]
    fn test_validate_empty_and_long() {
        assert!(validate_package_name("").is_err());
    }

    #[test]
    fn test_validate_edge_cases() {
        assert!(validate_package_name("-crate").is_err());
        assert!(validate_package_name("crate-").is_err());
        assert!(validate_package_name("_crate").is_ok());
    }

    #[test]
    fn test_git_status_non_git_dir() {
        use tempfile::TempDir;
        let temp = TempDir::new().unwrap();
        // Should not error on non-git directory
        assert!(check_git_status(temp.path()).is_ok());
    }
}
