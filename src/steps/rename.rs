//! Orchestration logic for package rename operations.
//!
//! All file system modifications go through a `Transaction` for atomicity.

use crate::cargo::{update_dependent_manifest, update_package_name, update_workspace_manifest};
use crate::error::{RenameError, Result};
use crate::fs::transaction::Transaction;
use crate::rewrite::update_source_code;
use crate::verify::{confirm_operation, preflight_checks};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use colored::Colorize;
use std::path::{Path, PathBuf};

/// Arguments for the `rename` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct RenameArgs {
    /// Current name of the package to rename
    pub old_name: String,

    /// New name for the package
    pub new_name: String,

    /// Move the package to a new directory
    ///
    /// When specified without a value, the directory is renamed to match
    /// the new package name (keeping the same parent directory).
    ///
    /// When specified with a path, the package is moved to that location
    /// (relative to workspace root).
    ///
    /// Examples:
    ///   --move                 Rename directory to match new package name
    ///   --move custom-name     Move to ./custom-name/
    ///   --move crates/api      Move to ./crates/api/
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    pub r#move: Option<Option<PathBuf>>,

    /// Path to workspace Cargo.toml
    ///
    /// If not specified, searches for Cargo.toml in the current directory
    /// and parent directories.
    #[arg(long, value_name = "PATH")]
    pub manifest_path: Option<PathBuf>,

    /// Preview changes without applying them
    #[arg(long, short = 'n')]
    pub dry_run: bool,

    /// Skip interactive confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Allow operation even with uncommitted git changes
    #[arg(long)]
    pub allow_dirty: bool,
}

impl RenameArgs {
    /// Returns `true` if the package should be moved to a different directory.
    pub fn should_move(&self) -> bool {
        self.r#move.is_some()
    }

    /// Calculates the new directory path for the package.
    ///
    /// # Arguments
    ///
    /// - `old_dir`: Current package directory (absolute path)
    /// - `workspace_root`: Workspace root directory (absolute path)
    ///
    /// # Returns
    ///
    /// - `Some(path)` if `--move` was specified
    /// - `None` if package should stay in same directory
    ///
    /// # Behavior
    ///
    /// - `--move` without argument: Renames directory to `new_name` in the same parent
    /// - `--move <path>`: Moves to `workspace_root/<path>`
    ///
    /// # Examples
    ///
    /// ```text
    /// Old: /workspace/crates/old-pkg
    ///
    /// --move              → /workspace/crates/new-pkg  (renamed in place)
    /// --move new-pkg      → /workspace/new-pkg         (moved to root)
    /// --move libs/api     → /workspace/libs/api        (moved to libs/)
    /// ```
    pub fn calculate_new_dir(&self, old_dir: &Path, workspace_root: &Path) -> Option<PathBuf> {
        if !self.should_move() {
            return None;
        }

        Some(match &self.r#move {
            Some(Some(custom_path)) => {
                // User provided explicit path - relative to workspace root
                workspace_root.join(custom_path)
            }
            Some(None) => {
                // --move without argument: rename directory in place
                old_dir
                    .parent()
                    .unwrap_or(workspace_root)
                    .join(&self.new_name)
            }
            None => unreachable!("should_move() returned true but r#move is None"),
        })
    }
}

/// Executes a package rename operation.
///
/// This is the main entry point for the rename command. It coordinates
/// all phases of the operation and ensures atomicity via transactions.
///
/// # Phases
///
/// 1. **Load metadata**: Parse workspace structure via `cargo metadata`
/// 2. **Pre-flight checks**: Validate names, check git status, verify package exists
/// 3. **User confirmation**: Show plan and wait for approval (unless `--yes`)
/// 4. **Stage operations**: Build transaction with all file modifications
/// 5. **Commit**: Execute all operations atomically
/// 6. **Verify**: Check workspace is still valid with `cargo metadata`
///
/// # Errors
///
/// - Returns error immediately if any phase fails
/// - Attempts automatic rollback if commit fails
/// - Never leaves workspace in partially-modified state
///
/// # Examples
///
/// ```no_run
/// # use cargo_rename::steps::rename::{execute, RenameArgs};
/// # fn example() -> cargo_rename::Result<()> {
/// let args = RenameArgs {
///     old_name: "crate-oldname".to_string(),
///     new_name: "crate-renamed".to_string(),
///     r#move: None,
///     manifest_path: None,
///     dry_run: true,
///     allow_dirty: false,
///     yes: false,
/// };
/// execute(args)?;
/// # Ok(())
/// # }
/// ```
pub fn execute(args: RenameArgs) -> Result<()> {
    // Phase 1: Load workspace metadata
    let metadata = load_metadata(&args)?;

    // Phase 2: Pre-flight checks
    preflight_checks(&args, &metadata)?;

    // Phase 3: User confirmation
    if !confirm_operation(&args, &metadata)? {
        println!("\n{}", "Operation cancelled.".yellow());
        return Err(RenameError::Cancelled);
    }

    // Get target package details
    let target_pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .ok_or_else(|| RenameError::PackageNotFound(args.old_name.clone()))?;

    let old_manifest_path = target_pkg.manifest_path.as_std_path();
    let old_dir = old_manifest_path.parent().unwrap();

    log::debug!(
        "Package '{}' is located at: {}",
        args.old_name,
        old_dir.display()
    );

    // Calculate new directory
    let new_dir = args
        .calculate_new_dir(old_dir, metadata.workspace_root.as_std_path())
        .unwrap_or_else(|| old_dir.to_path_buf());

    log::debug!("New directory will be: {}", new_dir.display());

    // Determine what's changing
    let name_changed = args.old_name != args.new_name;
    let path_changed = old_dir != new_dir;

    if !name_changed && !path_changed {
        return Err(RenameError::Other(anyhow::anyhow!(
            "Nothing to do: name and path are unchanged"
        )));
    }

    // Phase 4: Stage operations in transaction
    let mut txn = Transaction::new(args.dry_run);

    if let Err(e) = stage_rename_operations(
        &args,
        &metadata,
        old_manifest_path,
        old_dir,
        &new_dir,
        name_changed,
        path_changed,
        &mut txn,
    ) {
        return handle_staging_error(e, txn, &args);
    }

    // Phase 5: Commit transaction
    if let Err(e) = txn.commit() {
        return handle_commit_error(e, &mut txn, &args);
    }

    // Phase 6: Post-commit verification
    if !args.dry_run {
        verify_workspace(metadata.workspace_root.as_std_path(), path_changed)?;
    }

    // Print summary
    txn.print_summary(
        &args.old_name,
        &args.new_name,
        metadata.workspace_root.as_std_path(),
    );

    // Success message
    if !args.dry_run {
        println!(
            "\n{} {} → {}",
            "✓ Successfully renamed".green().bold(),
            args.old_name.yellow(),
            args.new_name.green().bold()
        );
    }

    Ok(())
}

/// Loads Cargo workspace metadata.
///
/// # Arguments
///
/// - `args`: Command arguments (may contain custom manifest path)
///
/// # Errors
///
/// - `Io(NotFound)`: Manifest path doesn't exist
/// - `Metadata`: `cargo metadata` command failed
fn load_metadata(args: &RenameArgs) -> Result<cargo_metadata::Metadata> {
    let mut cmd = MetadataCommand::new();

    if let Some(path) = &args.manifest_path {
        if !path.exists() {
            return Err(RenameError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Manifest path does not exist: {}", path.display()),
            )));
        }

        if path.is_dir() {
            return Err(RenameError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Manifest path is a directory, expected file: {}",
                    path.display()
                ),
            )));
        }

        cmd.manifest_path(path);
    }

    cmd.exec().map_err(|e| {
        RenameError::Other(anyhow::anyhow!(
            "Failed to load workspace metadata: {}. Is this a valid Cargo workspace?",
            e
        ))
    })
}

/// Stages all rename operations in a transaction.
///
/// Operations are staged in dependency order:
/// 1. Directory move (staged first, executed last during commit)
/// 2. Update target package's own manifest
/// 3. Update dependent packages' manifests
/// 4. Update workspace manifest
/// 5. Update source code references
///
/// No file system modifications occur until `txn.commit()` is called.
#[allow(clippy::too_many_arguments)]
fn stage_rename_operations(
    args: &RenameArgs,
    metadata: &cargo_metadata::Metadata,
    old_manifest_path: &Path,
    old_dir: &Path,
    new_dir: &Path,
    name_changed: bool,
    path_changed: bool,
    txn: &mut Transaction,
) -> Result<()> {
    // Stage directory move (executed last during commit)
    if path_changed {
        log::info!(
            "Staging directory move: {} → {}",
            old_dir.display(),
            new_dir.display()
        );
        txn.move_directory(old_dir.to_path_buf(), new_dir.to_path_buf())?;
    }

    // Update target package's manifest
    if name_changed {
        log::info!("Updating package name in {}", old_manifest_path.display());
        update_package_name(old_manifest_path, &args.new_name, txn)?;
    }

    // Update dependent packages
    log::info!("Updating dependent manifests...");
    let target_pkg_id = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .map(|p| &p.id)
        .unwrap();

    for member_id in &metadata.workspace_members {
        if member_id == target_pkg_id {
            continue; // Skip target package
        }

        let member = &metadata[member_id];

        // Check if this member depends on the target
        let has_dependency = member
            .dependencies
            .iter()
            .any(|d| d.name == args.old_name || d.rename.as_deref() == Some(&args.old_name));

        if !has_dependency {
            log::debug!(
                "Skipping {} (no dependency on {})",
                member.name,
                args.old_name
            );
            continue;
        }

        log::debug!(
            "Updating manifest: {}",
            member.manifest_path.as_std_path().display()
        );
        update_dependent_manifest(
            member.manifest_path.as_std_path(),
            &args.old_name,
            &args.new_name,
            new_dir,
            path_changed,
            name_changed,
            txn,
        )?;
    }

    // Update workspace manifest
    log::info!("Updating workspace manifest...");
    let root_manifest = metadata.workspace_root.as_std_path().join("Cargo.toml");
    if root_manifest.exists() {
        let should_update_members = path_changed;

        if should_update_members || name_changed {
            update_workspace_manifest(
                &root_manifest,
                &args.old_name,
                &args.new_name,
                old_dir,
                new_dir,
                should_update_members,
                path_changed,
                name_changed,
                txn,
            )?;
        }
    }

    // Update source code
    if name_changed {
        log::info!("Updating source code references...");
        update_source_code(metadata, &args.old_name, &args.new_name, txn)?;
    }

    log::debug!(
        "All operations staged successfully ({} operations)",
        txn.len()
    );

    Ok(())
}

/// Handles errors that occur during operation staging.
///
/// No rollback is needed since operations haven't been committed yet.
fn handle_staging_error(e: RenameError, txn: Transaction, args: &RenameArgs) -> Result<()> {
    eprintln!("{} {}", "Error during rename:".red().bold(), e);

    if !args.dry_run && !txn.is_empty() {
        eprintln!("{} No changes were committed.", "ℹ".blue().bold());
    }

    Err(e)
}

/// Handles errors that occur during transaction commit.
///
/// Attempts automatic rollback if changes were partially applied.
fn handle_commit_error(e: RenameError, txn: &mut Transaction, args: &RenameArgs) -> Result<()> {
    eprintln!("{} {}", "Error during commit:".red().bold(), e);
    eprintln!("Some operations may have been applied.");

    // Attempt rollback
    if !args.dry_run && txn.is_committed() {
        eprintln!("{}", "Attempting to rollback changes...".yellow().bold());

        match txn.rollback() {
            Ok(_) => {
                eprintln!("{}", "✓ Rollback successful. Workspace restored.".green());
            }
            Err(rollback_err) => {
                eprintln!("{} {}", "✗ Rollback failed:".red().bold(), rollback_err);
                eprintln!(
                    "{}",
                    "⚠ Manual intervention may be required to restore workspace."
                        .yellow()
                        .bold()
                );
                eprintln!("Hint: Check your version control system for recovery.");
            }
        }
    }

    Err(e)
}

/// Verifies the workspace is still valid after rename.
///
/// Runs `cargo metadata` to check that Cargo can still parse the workspace.
/// Logs warnings but doesn't fail the operation (rename already succeeded).
fn verify_workspace(workspace_root: &Path, structure_changed: bool) -> Result<()> {
    log::info!("Verifying workspace structure...");

    let output = std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--no-deps")
        .current_dir(workspace_root)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            log::info!("✓ Workspace verification passed");
            Ok(())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("Workspace verification failed:");
            log::error!("{}", stderr);

            if structure_changed {
                log::warn!("The rename completed but the workspace structure may have issues.");
                log::warn!("Try running 'cargo check' to diagnose the problem.");
            }

            // Don't return error - rename succeeded, just workspace might need manual fix
            Ok(())
        }
        Err(e) => {
            log::warn!("Could not verify workspace: {}", e);
            log::warn!("The rename may have succeeded, but verification failed.");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_should_move() {
        let mut args = RenameArgs {
            old_name: "old".into(),
            new_name: "new".into(),
            r#move: None,
            manifest_path: None,
            dry_run: false,
            yes: false,
            allow_dirty: false,
        };

        assert!(!args.should_move());

        args.r#move = Some(None);
        assert!(args.should_move());

        args.r#move = Some(Some(PathBuf::from("custom")));
        assert!(args.should_move());
    }

    #[test]
    fn test_calculate_new_dir_no_move() {
        let workspace = Path::new("/workspace");
        let old_dir = workspace.join("crates/old-pkg");

        let args = RenameArgs {
            old_name: "old-pkg".into(),
            new_name: "new-pkg".into(),
            r#move: None,
            manifest_path: None,
            dry_run: false,
            yes: false,
            allow_dirty: false,
        };

        assert_eq!(args.calculate_new_dir(&old_dir, workspace), None);
    }

    #[test]
    fn test_calculate_new_dir_rename_in_place() {
        let workspace = Path::new("/workspace");
        let old_dir = workspace.join("crates/old-pkg");

        let args = RenameArgs {
            old_name: "old-pkg".into(),
            new_name: "new-pkg".into(),
            r#move: Some(None), // --move without argument
            manifest_path: None,
            dry_run: false,
            yes: false,
            allow_dirty: false,
        };

        // Should keep parent directory (crates/)
        assert_eq!(
            args.calculate_new_dir(&old_dir, workspace),
            Some(workspace.join("crates/new-pkg"))
        );
    }

    #[test]
    fn test_calculate_new_dir_custom_path() {
        let workspace = Path::new("/workspace");
        let old_dir = workspace.join("crates/old-pkg");

        let args = RenameArgs {
            old_name: "old-pkg".into(),
            new_name: "new-pkg".into(),
            r#move: Some(Some(PathBuf::from("libs/api"))),
            manifest_path: None,
            dry_run: false,
            yes: false,
            allow_dirty: false,
        };

        assert_eq!(
            args.calculate_new_dir(&old_dir, workspace),
            Some(workspace.join("libs/api"))
        );
    }

    #[test]
    fn test_calculate_new_dir_at_workspace_root() {
        let workspace = Path::new("/workspace");
        let old_dir = workspace.join("old-pkg");

        let args = RenameArgs {
            old_name: "old-pkg".into(),
            new_name: "new-pkg".into(),
            r#move: Some(None),
            manifest_path: None,
            dry_run: false,
            yes: false,
            allow_dirty: false,
        };

        // Should still work when parent is workspace root
        assert_eq!(
            args.calculate_new_dir(&old_dir, workspace),
            Some(workspace.join("new-pkg"))
        );
    }
}
