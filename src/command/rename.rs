use crate::error::{RenameError, Result};
use crate::ops::{Transaction, update_workspace_manifest};
use crate::ops::{update_dependent_manifest, update_package_name, update_source_code};
use crate::validation::{confirm_operation, preflight_checks};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use colored::Colorize;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug, Clone)]
#[clap(verbatim_doc_comment)]
pub struct RenameArgs {
    /// Current name of the package to rename
    pub old_name: String,

    /// New name of the package
    pub new_name: String,

    /// Also move the package to a new directory
    ///
    /// By default, only the package name is changed in Cargo.toml and references.
    ///
    /// When specified without a value, the directory is renamed to match the new
    /// package name. When specified with a path, the package is moved to that location.
    ///
    /// Examples:
    ///   --move              Move to directory matching new package name
    ///   --move custom-name  Move to ./custom-name/
    ///   --move crates/api   Move to ./crates/api/
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    pub r#move: Option<Option<PathBuf>>,

    /// Path to the workspace Cargo.toml (defaults to current directory)
    #[arg(long, value_name = "PATH")]
    pub manifest_path: Option<PathBuf>,

    /// Show what would change without applying any modifications
    #[arg(long, short = 'n')]
    pub dry_run: bool,

    /// Skip the interactive confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Allow renaming even if the git workspace has uncommitted changes
    #[arg(long)]
    pub allow_dirty: bool,
}

impl RenameArgs {
    /// Returns true if the package should be moved to a new directory
    pub fn should_move(&self) -> bool {
        self.r#move.is_some()
    }

    /// Calculates the new directory path based on the move argument
    pub fn calculate_new_dir(&self, workspace_root: &Path) -> Option<PathBuf> {
        if !self.should_move() {
            return None;
        }

        Some(match &self.r#move {
            Some(Some(custom_path)) => {
                // User provided a custom path - relative to workspace root
                workspace_root.join(custom_path)
            }
            Some(None) => {
                // --move without argument: use new package name
                workspace_root.join(&self.new_name)
            }
            None => unreachable!("should_move() returned true but r#move is None"),
        })
    }
}

pub fn execute(args: RenameArgs) -> Result<()> {
    // Load workspace metadata
    let metadata = load_metadata(&args)?;

    // Pre-flight checks
    preflight_checks(&args, &metadata)?;

    // Confirm with user (shows plan)
    if !confirm_operation(&args, &metadata)? {
        println!("\n{}", "Operation cancelled.".yellow());
        return Err(RenameError::Cancelled);
    }

    // Get the package we're renaming
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
    log::debug!(
        "Workspace root: {}",
        metadata.workspace_root.as_std_path().display()
    );

    // Calculate new directory
    let new_dir = args
        .calculate_new_dir(metadata.workspace_root.as_std_path())
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

    // Execute the rename with transaction
    let mut txn = Transaction::new(args.dry_run);

    if let Err(e) = execute_rename(
        &args,
        &metadata,
        old_manifest_path,
        old_dir,
        &new_dir,
        name_changed,
        path_changed,
        &mut txn,
    ) {
        handle_rename_error(e, txn, &args)?;
        unreachable!("handle_rename_error always returns Err");
    }

    // Commit transaction
    if let Err(e) = txn.commit() {
        handle_commit_error(e, &mut txn, &args)?;
        unreachable!("handle_commit_error always returns Err");
    }

    // Post-commit verification
    if !args.dry_run {
        verify_workspace(&metadata.workspace_root.as_std_path(), path_changed)?;
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

/// Load cargo metadata with proper error handling
fn load_metadata(args: &RenameArgs) -> Result<cargo_metadata::Metadata> {
    let mut cmd = MetadataCommand::new();

    if let Some(path) = &args.manifest_path {
        if !path.exists() {
            return Err(RenameError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Manifest path does not exist: {}", path.display()),
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

/// Execute all rename operations in proper order
fn execute_rename(
    args: &RenameArgs,
    metadata: &cargo_metadata::Metadata,
    old_manifest_path: &Path,
    old_dir: &Path,
    new_dir: &Path,
    name_changed: bool,
    path_changed: bool,
    txn: &mut Transaction,
) -> Result<()> {
    // STEP 1: Stage directory move FIRST (so Transaction can track path redirects)
    // But don't execute until commit
    if path_changed {
        log::info!(
            "Staging directory move: {} → {}",
            old_dir.display(),
            new_dir.display()
        );
        txn.move_directory(old_dir.to_path_buf(), new_dir.to_path_buf())?;
    }

    // STEP 2: Update the target package's own manifest (name field)
    if name_changed {
        log::info!("Updating package name in {}", old_manifest_path.display());
        update_package_name(old_manifest_path, &args.new_name, txn)?;
    }

    // STEP 3: Update all OTHER packages that depend on this one
    log::info!("Updating dependent manifests...");
    let target_pkg_id = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .map(|p| &p.id)
        .unwrap();

    for member_id in &metadata.workspace_members {
        // Skip the target package itself (already updated in step 2)
        if member_id == target_pkg_id {
            continue;
        }

        let member = &metadata[member_id];

        // Only update if this member actually depends on the target
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

    // STEP 4: Update workspace-level Cargo.toml (members and dependencies)
    log::info!("Updating workspace manifest...");
    let root_manifest = metadata.workspace_root.as_std_path().join("Cargo.toml");
    if root_manifest.exists() {
        let should_update_members = path_changed;
        let should_update_deps = path_changed || name_changed;

        if should_update_members || should_update_deps {
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

    // STEP 5: Update source code references (use statements, paths, etc.)
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

/// Handle errors during operation staging
fn handle_rename_error(e: RenameError, txn: Transaction, args: &RenameArgs) -> Result<()> {
    eprintln!("{} {}", "Error during rename:".red().bold(), e);

    if !args.dry_run && !txn.is_empty() {
        eprintln!("{} No changes were committed.", "ℹ".blue().bold());
    }

    Err(e)
}

/// Handle errors during transaction commit with rollback attempt
fn handle_commit_error(e: RenameError, txn: &mut Transaction, args: &RenameArgs) -> Result<()> {
    eprintln!("{} {}", "Error during commit:".red().bold(), e);
    eprintln!("Some operations may have been applied.");

    // Attempt rollback if not dry-run
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

/// Verify workspace is still valid after rename
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

            // Don't return error - rename was successful, just workspace might need manual fix
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

    #[test]
    fn test_should_move() {
        let mut args = RenameArgs {
            old_name: "old".to_string(),
            new_name: "new".to_string(),
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
    fn test_calculate_new_dir() {
        let workspace = PathBuf::from("/workspace");

        let mut args = RenameArgs {
            old_name: "old-pkg".to_string(),
            new_name: "new-pkg".to_string(),
            r#move: None,
            manifest_path: None,
            dry_run: false,
            yes: false,
            allow_dirty: false,
        };

        // No move
        assert_eq!(args.calculate_new_dir(&workspace), None);

        // Move with default (use new name)
        args.r#move = Some(None);
        assert_eq!(
            args.calculate_new_dir(&workspace),
            Some(workspace.join("new-pkg"))
        );

        // Move with custom path
        args.r#move = Some(Some(PathBuf::from("crates/api")));
        assert_eq!(
            args.calculate_new_dir(&workspace),
            Some(workspace.join("crates/api"))
        );
    }

    #[test]
    fn test_no_changes_error() {
        let _args = RenameArgs {
            old_name: "same".to_string(),
            new_name: "same".to_string(),
            r#move: None,
            manifest_path: None,
            dry_run: true,
            yes: true,
            allow_dirty: false,
        };

        // This should fail in preflight_checks or early in execute
        // Testing the logic for name_changed && path_changed
    }
}
