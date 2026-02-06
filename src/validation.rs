use crate::command::rename::RenameArgs;
use crate::error::{RenameError, Result};
use cargo_metadata::Metadata;
use colored::Colorize;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

/// Maximum allowed package name length (Cargo's limit)
const MAX_PACKAGE_NAME_LENGTH: usize = 64;

/// Validates a package name according to Cargo's naming rules.
///
/// Rules:
/// - Must start with an ASCII letter or underscore
/// - Can only contain ASCII alphanumerics, hyphens, and underscores
/// - Cannot be empty
/// - Cannot exceed 64 characters
/// - Cannot be a reserved name (test, doc, build, bench)
/// - Cannot start or end with hyphen
/// - Should not conflict with existing crates.io normalization
///
/// Errors:
/// Returns `RenameError::InvalidName` if validation fails.
pub fn validate_package_name(name: &str) -> Result<()> {
    // Check empty first - most basic validation
    if name.is_empty() {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot be empty".to_string(),
        ));
    }

    // Check length
    if name.len() > MAX_PACKAGE_NAME_LENGTH {
        return Err(RenameError::InvalidName(
            name.to_string(),
            format!(
                "exceeds maximum length of {} characters (has {})",
                MAX_PACKAGE_NAME_LENGTH,
                name.len()
            ),
        ));
    }

    // Check first character is ASCII letter or underscore
    let first_char = name.chars().next().unwrap(); // Safe: we checked non-empty
    if !first_char.is_ascii_alphabetic() && first_char != '_' {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "must start with an ASCII letter or underscore".to_string(),
        ));
    }

    // Check all characters are valid (ASCII only)
    for (idx, ch) in name.chars().enumerate() {
        if !ch.is_ascii() {
            return Err(RenameError::InvalidName(
                name.to_string(),
                format!(
                    "contains non-ASCII character '{}' at position {}. Only ASCII characters are allowed",
                    ch, idx
                ),
            ));
        }

        if !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-' {
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
                "'{}' is a reserved package name. Reserved names: {}",
                name,
                RESERVED.join(", ")
            ),
        ));
    }

    // Check for hyphens at start/end
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

    // Check for consecutive hyphens (bad practice)
    if name.contains("--") {
        log::warn!(
            "Package name '{}' contains consecutive hyphens, which may cause confusion",
            name
        );
    }

    // Warn about potential crates.io conflicts
    // On crates.io, my-crate and my_crate are considered the SAME package
    if name.contains('_') && name.contains('-') {
        log::warn!(
            "Package name '{}' contains both underscores and hyphens. This is valid but may cause confusion.",
            name
        );
    }

    // Warn about naming convention
    if name.chars().any(|c| c.is_ascii_uppercase()) {
        log::warn!(
            "Package name '{}' contains uppercase letters. By convention, package names should be lowercase with hyphens.",
            name
        );
    }

    Ok(())
}

/// Validates a directory name/path for the --move flag
///
/// Rules:
/// - Cannot be empty
/// - Cannot contain invalid path characters
/// - Must be relative (not absolute)
/// - Cannot navigate outside workspace
/// - Cannot be just "." or ".."
/// - Must not contain path traversal sequences
///
/// Errors:
/// Returns `RenameError::InvalidName` if validation fails.
pub fn validate_directory_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "directory name cannot be empty".to_string(),
        ));
    }

    let path = Path::new(name);

    // Check for absolute paths (platform-aware)
    if path.is_absolute() {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "directory must be a relative path, not absolute".to_string(),
        ));
    }

    // Additional check: Unix-style absolute paths on Windows
    // Also check for UNC paths
    #[cfg(windows)]
    {
        if name.starts_with('/') || name.starts_with('\\') {
            return Err(RenameError::InvalidName(
                name.to_string(),
                "directory must be a relative path, not absolute".to_string(),
            ));
        }

        // Check for UNC paths: \\server\share
        if name.starts_with(r"\\") {
            return Err(RenameError::InvalidName(
                name.to_string(),
                "UNC paths are not allowed".to_string(),
            ));
        }
    }

    // Check for dangerous navigation
    if name == "." || name == ".." {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot use '.' or '..' as directory name".to_string(),
        ));
    }

    // Check each path component for parent directory traversal
    use std::path::Component;
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(RenameError::InvalidName(
                    name.to_string(),
                    "cannot navigate outside workspace using '..'".to_string(),
                ));
            }
            Component::CurDir => {
                // Allow current dir in middle of path: "foo/./bar" is valid
            }
            Component::Normal(_) => {
                // Normal path component is fine
            }
            _ => {
                // Prefix, RootDir should have been caught by is_absolute check
                return Err(RenameError::InvalidName(
                    name.to_string(),
                    "invalid path component".to_string(),
                ));
            }
        }
    }

    // Check for null bytes (security)
    if name.contains('\0') {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "directory name cannot contain null bytes".to_string(),
        ));
    }

    // Platform-specific checks
    #[cfg(windows)]
    {
        // Windows invalid characters: < > : " | ? *
        // Note: / and \ are valid path separators
        const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*'];
        for ch in INVALID_CHARS {
            if name.contains(*ch) {
                return Err(RenameError::InvalidName(
                    name.to_string(),
                    format!("directory name cannot contain '{}'", ch),
                ));
            }
        }

        // Windows reserved names (check each path component)
        const RESERVED: &[&str] = &[
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];

        for component in path.components() {
            if let Component::Normal(os_str) = component {
                if let Some(component_str) = os_str.to_str() {
                    // Extract just the name without extension for reserved name check
                    let name_part = if let Some(dot_pos) = component_str.rfind('.') {
                        &component_str[..dot_pos]
                    } else {
                        component_str
                    };

                    let name_upper = name_part.to_uppercase();
                    if RESERVED.contains(&name_upper.as_str()) {
                        return Err(RenameError::InvalidName(
                            name.to_string(),
                            format!("'{}' is a reserved name on Windows", component_str),
                        ));
                    }
                }
            }
        }
    }

    // Check for trailing dots or spaces (Windows issue, but good practice everywhere)
    for component in path.components() {
        if let Component::Normal(os_str) = component {
            if let Some(s) = os_str.to_str() {
                if s.ends_with('.') || s.ends_with(' ') {
                    return Err(RenameError::InvalidName(
                        name.to_string(),
                        format!("path component '{}' cannot end with '.' or space", s),
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Validates that directory path is within workspace bounds
pub fn validate_directory_within_workspace(dir_path: &Path, workspace_root: &Path) -> Result<()> {
    // Construct the full path
    let full_path = workspace_root.join(dir_path);

    // Try to canonicalize to resolve any .. or symlinks
    // Note: This might fail if path doesn't exist yet, which is OK
    if let Ok(canonical) = full_path.canonicalize() {
        let canonical_workspace = workspace_root.canonicalize().map_err(|e| {
            RenameError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to canonicalize workspace root: {}", e),
            ))
        })?;

        if !canonical.starts_with(&canonical_workspace) {
            return Err(RenameError::InvalidName(
                dir_path.display().to_string(),
                "resolved path is outside workspace".to_string(),
            ));
        }
    } else {
        // Path doesn't exist yet - check components manually
        // Just ensure it doesn't have .. that would escape
        // (already checked in validate_directory_name)
    }

    Ok(())
}

/// Checks if the git working directory has uncommitted changes.
///
/// Behavior:
/// - Returns error if workspace has uncommitted **tracked** changes
/// - Ignores untracked files (new files that aren't in git)
/// - Returns Ok if workspace is clean
/// - Returns Ok if not a git repository (fails silently)
/// - Returns Ok if git is not installed (fails silently)
///
/// Errors:
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
    // Using -uno to ignore untracked files
    match Command::new("git")
        .args(["status", "--porcelain", "-uno"]) // -uno = don't show untracked files
        .current_dir(workspace_root)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
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
            } else {
                log::warn!(
                    "Git status command failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                Ok(()) // Fail silently
            }
        }
        Err(e) => {
            log::warn!("Failed to execute git status: {}", e);
            Ok(()) // Fail silently
        }
    }
}

/// Performs all pre-flight checks before executing rename operation.
///
/// Checks:
/// 1. Validates new package name
/// 2. Validates directory name (if --move is specified)
/// 3. Verifies old package exists
/// 4. Checks new name differs from old name (if not moving)
/// 5. Checks target directory doesn't already exist (if moving)
/// 6. Validates directory is within workspace bounds
/// 7. Checks git status (unless --allow-dirty)
///
/// Errors:
/// Returns first error encountered during checks.
pub fn preflight_checks(args: &RenameArgs, metadata: &Metadata) -> Result<()> {
    // 1. Validate new package name
    validate_package_name(&args.new_name)?;

    // 2. Validate directory name if --move is specified with custom path
    if let Some(Some(custom_path)) = &args.r#move {
        if let Some(path_str) = custom_path.to_str() {
            validate_directory_name(path_str)?;
            validate_directory_within_workspace(
                custom_path,
                metadata.workspace_root.as_std_path(),
            )?;
        } else {
            return Err(RenameError::InvalidName(
                custom_path.display().to_string(),
                "directory path contains invalid UTF-8".to_string(),
            ));
        }
    }

    // 3. Verify old package exists
    let _pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .ok_or_else(|| RenameError::PackageNotFound(args.old_name.clone()))?;

    // 4. Check git status (unless --allow-dirty)
    if !args.allow_dirty {
        if let Err(e) = check_git_status(metadata.workspace_root.as_std_path()) {
            log::error!("{}", e);
            log::info!("Hint: Use --allow-dirty to bypass this check");
            return Err(e);
        }
    }

    // 5. Additional safety check: ensure new name differs from old name
    if args.old_name == args.new_name && !args.should_move() {
        return Err(RenameError::Other(anyhow::anyhow!(
            "New name '{}' is the same as the old name and --move was not specified. Nothing to do.",
            args.new_name
        )));
    }

    // 6. Check if target directory would conflict (if moving)
    if args.should_move() {
        let new_dir = args
            .calculate_new_dir(metadata.workspace_root.as_std_path())
            .unwrap();

        // Note: This is a TOCTOU check - directory could be created between now and commit
        // The Transaction will perform a final check atomically
        if new_dir.exists() {
            return Err(RenameError::DirectoryExists(new_dir.to_path_buf()));
        }

        // Additional check: ensure parent directory exists or can be created
        if let Some(parent) = new_dir.parent() {
            if !parent.exists() {
                log::info!("Parent directory '{}' will be created", parent.display());
            }
        }
    }

    Ok(())
}

/// Prompts user for confirmation before executing rename operation.
///
/// Behavior:
/// - Skips prompt if --yes or --dry-run flag is set
/// - Shows detailed plan of changes
/// - Waits for user input (with timeout on non-interactive terminals)
///
/// Returns:
/// - Ok(true) if user confirms or prompt is skipped
/// - Ok(false) if user declines
/// - Err if IO error occurs
pub fn confirm_operation(args: &RenameArgs, metadata: &Metadata) -> Result<bool> {
    // Skip confirmation if flags are set
    if args.yes || args.dry_run {
        return Ok(true);
    }

    // Check if stdin is a terminal (not redirected)
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        if !unsafe { libc::isatty(std::io::stdin().as_raw_fd()) != 0 } {
            log::warn!("Non-interactive terminal detected. Use --yes to confirm automatically.");
            return Ok(false);
        }
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

    // Show what will be updated
    println!("  {} Update package name in Cargo.toml", "✓".green());
    println!("  {} Update source code references", "✓".green());
    println!("  {} Update workspace dependencies", "✓".green());

    // Show move operation details
    if args.should_move() {
        let old_dir = pkg.manifest_path.parent().unwrap();
        let new_dir = args
            .calculate_new_dir(metadata.workspace_root.as_std_path())
            .unwrap();
        let old_dir_name = old_dir.file_name().unwrap().to_string();
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

    // Show dependents
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

    // Prompt for confirmation
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
        assert!(validate_package_name("_private").is_ok());
    }

    #[test]
    fn test_validate_invalid_start() {
        assert!(validate_package_name("123crate").is_err());
        assert!(validate_package_name("-crate").is_err());
    }

    #[test]
    fn test_validate_invalid_chars() {
        assert!(validate_package_name("my crate").is_err());
        assert!(validate_package_name("my.crate").is_err());
        assert!(validate_package_name("my@crate").is_err());
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
    fn test_validate_empty_and_edge_cases() {
        assert!(validate_package_name("").is_err());
        assert!(validate_package_name("-crate").is_err());
        assert!(validate_package_name("crate-").is_err());
        assert!(validate_package_name("_crate").is_ok());
    }

    #[test]
    fn test_validate_length_limit() {
        let too_long = "a".repeat(65);
        assert!(validate_package_name(&too_long).is_err());

        let max_length = "a".repeat(64);
        assert!(validate_package_name(&max_length).is_ok());
    }

    #[test]
    fn test_validate_non_ascii() {
        assert!(validate_package_name("café").is_err());
        assert!(validate_package_name("テスト").is_err());
    }

    #[test]
    fn test_validate_directory_names() {
        // Valid directory names
        assert!(validate_directory_name("my-dir").is_ok());
        assert!(validate_directory_name("crates/api").is_ok());
        assert!(validate_directory_name("crates/backend/api-v2").is_ok());
        assert!(validate_directory_name("_private").is_ok());

        // Invalid directory names
        assert!(validate_directory_name("").is_err());
        assert!(validate_directory_name(".").is_err());
        assert!(validate_directory_name("..").is_err());
        assert!(validate_directory_name("../../../etc/passwd").is_err());
        assert!(validate_directory_name("crates/../secrets").is_err());
        assert!(validate_directory_name("foo/../bar").is_err()); // Any .. component

        // Absolute paths
        assert!(validate_directory_name("/absolute/path").is_err());

        #[cfg(windows)]
        {
            assert!(validate_directory_name("C:\\absolute").is_err());
            assert!(validate_directory_name("\\\\server\\share").is_err()); // UNC path
            assert!(validate_directory_name("CON").is_err());
            assert!(validate_directory_name("PRN").is_err());
            assert!(validate_directory_name("dir<name").is_err());
            assert!(validate_directory_name("dir>name").is_err());
            assert!(validate_directory_name("dir:name").is_err());
            assert!(validate_directory_name("dir|name").is_err());
        }
    }

    #[test]
    fn test_git_status_non_git_dir() {
        use tempfile::TempDir;
        let temp = TempDir::new().unwrap();

        // Should not error on non-git directory
        assert!(check_git_status(temp.path()).is_ok());
    }

    #[test]
    fn test_consecutive_hyphens() {
        // Should succeed but warn
        assert!(validate_package_name("my--crate").is_ok());
    }
}
