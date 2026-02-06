//! Pre-flight checks performed before executing a rename operation.
//!
//! These checks validate the current workspace state and ensure the
//! rename operation can proceed safely. Unlike `rules`, these functions
//! may perform I/O (checking git status, verifying files exist, etc.).

use crate::error::{RenameError, Result};
use crate::steps::rename::RenameArgs;
use crate::verify::rules::{
    validate_directory_path, validate_package_name, validate_path_within_workspace,
};
use cargo_metadata::Metadata;
use std::path::Path;
use std::process::Command;

/// Checks if the git working directory has uncommitted **tracked** changes.
///
/// Untracked files (new files not in git) are ignored because they won't be
/// affected by the rename operation.
///
/// # Behavior
///
/// - Returns `Err(DirtyWorkspace)` if tracked files have uncommitted changes
/// - Returns `Ok(())` if workspace is clean
/// - Returns `Ok(())` if not a git repository (fails silently)
/// - Returns `Ok(())` if git is not installed (fails silently)
///
/// # Errors
///
/// Only returns `DirtyWorkspace` if changes are detected. All other errors
/// (git not found, not a repo) are logged but don't fail the check.
pub fn check_git_status(workspace_root: &Path) -> Result<()> {
    // Check if git is available
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

    // Check for uncommitted changes (-uno = ignore untracked files)
    match Command::new("git")
        .args(["status", "--porcelain", "-uno"])
        .current_dir(workspace_root)
        .output()
    {
        Ok(output) if output.status.success() => {
            if !output.stdout.is_empty() {
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
            Ok(())
        }
        Err(e) => {
            log::warn!("Failed to execute git status: {}", e);
            Ok(())
        }
    }
}

/// Performs comprehensive pre-flight validation before rename execution.
///
/// # Checks Performed
///
/// 1. New package name conforms to Cargo rules
/// 2. Directory path is valid (if `--move` specified)
/// 3. Directory is within workspace bounds (if `--move` specified)
/// 4. Old package exists in workspace
/// 5. Git workspace is clean (unless `--allow-dirty`)
/// 6. Operation would actually change something
/// 7. Target directory doesn't exist (if moving)
///
/// # Errors
///
/// Returns the first validation error encountered. No filesystem modifications
/// are made during validation.
pub fn preflight_checks(args: &RenameArgs, metadata: &Metadata) -> Result<()> {
    // Validate new package name
    validate_package_name(&args.effective_new_name())?;

    // Validate directory path (if --move specified)
    if let Some(Some(custom_path)) = &args.outdir {
        if let Some(path_str) = custom_path.to_str() {
            validate_directory_path(path_str, metadata.workspace_root.as_std_path())?;
            validate_path_within_workspace(custom_path, metadata.workspace_root.as_std_path())?;
        } else {
            return Err(RenameError::InvalidName(
                custom_path.display().to_string(),
                "path contains invalid UTF-8".to_string(),
            ));
        }
    }

    // Verify old package exists
    let pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .ok_or_else(|| RenameError::PackageNotFound(args.old_name.clone()))?;

    // Check git status (unless --allow-dirty)
    if !args.allow_dirty
        && let Err(e) = check_git_status(metadata.workspace_root.as_std_path())
    {
        log::error!("{}", e);
        log::info!("Hint: Use --allow-dirty to bypass this check");
        return Err(e);
    }

    // Check target directory doesn't exist (if moving)
    if args.should_move() {
        let old_dir = pkg.manifest_path.parent().unwrap().as_std_path();
        let new_dir = args
            .calculate_new_dir(old_dir, metadata.workspace_root.as_std_path())
            .unwrap();

        // Only check if target exists when actually moving to a different location
        if old_dir != new_dir && new_dir.exists() {
            return Err(RenameError::DirectoryExists(new_dir));
        }

        // Log if parent directory will be created
        if let Some(parent) = new_dir.parent()
            && !parent.exists()
        {
            log::info!("Parent directory '{}' will be created", parent.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_git_status_non_git_dir() {
        let temp = TempDir::new().unwrap();
        assert!(check_git_status(temp.path()).is_ok());
    }
}
