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

    /// Returns the target directory name for the move operation
    ///
    /// - If --move was specified without a value, returns the new package name
    /// - If --move was specified with a value, returns that value
    /// - If --move was not specified, returns None
    pub fn move_target(&self) -> Option<&str> {
        self.r#move.as_ref().map(|opt_path| {
            opt_path
                .as_ref()
                .and_then(|p| p.to_str())
                .unwrap_or(&self.new_name)
        })
    }

    /// Calculates the new directory path based on the current directory
    pub fn calculate_new_dir(&self, _old_dir: &Path, workspace_root: &Path) -> Option<PathBuf> {
        self.move_target().map(|target| workspace_root.join(target))
    }
}

pub fn execute(args: RenameArgs) -> Result<()> {
    // Load workspace metadata
    let mut cmd = MetadataCommand::new();
    if let Some(path) = &args.manifest_path {
        cmd.manifest_path(path);
    }
    let metadata = cmd.exec()?;

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
    let new_dir = if args.should_move() {
        match &args.r#move {
            Some(Some(custom_path)) => {
                // User provided a custom path - it should be relative to workspace root
                metadata.workspace_root.as_std_path().join(custom_path)
            }
            Some(None) => {
                // --move without argument: move to directory matching package name
                metadata.workspace_root.as_std_path().join(&args.new_name)
            }
            None => old_dir.to_path_buf(),
        }
    } else {
        old_dir.to_path_buf()
    };

    log::debug!("New directory will be: {}", new_dir.display());

    // Determine if we're actually moving
    let should_move = args.should_move() && old_dir != new_dir;

    // Create transaction
    let mut txn = Transaction::new(args.dry_run);

    // Execute operations
    let result: Result<()> = (|| {
        // 1. Update all dependent manifests
        for member_id in &metadata.workspace_members {
            let member = &metadata[member_id];
            update_dependent_manifest(
                member.manifest_path.as_std_path(),
                &args.old_name,
                &args.new_name,
                &new_dir,
                should_move, // path_changed
                true,        // name_changed (always true for rename)
                &mut txn,
            )?;
        }

        // 2. Update target package name
        update_package_name(old_manifest_path, &args.new_name, &mut txn)?;

        // 3. Update source code references
        update_source_code(&metadata, &args.old_name, &args.new_name, &mut txn)?;

        // 4. Update workspace manifest
        let root_manifest = metadata.workspace_root.as_std_path().join("Cargo.toml");
        if root_manifest.exists() {
            let name_changed = args.old_name != args.new_name;
            let path_changed = old_dir != new_dir;
            let should_update_members = should_move && old_dir != new_dir;
            let should_update_deps = old_dir != new_dir || name_changed;

            if should_update_members || should_update_deps {
                update_workspace_manifest(
                    &root_manifest,
                    &args.old_name,
                    &args.new_name,
                    old_dir,
                    &new_dir,
                    should_update_members,
                    path_changed,
                    name_changed,
                    &mut txn,
                )?;
            }
        }

        // 5. Move directory (last step)
        if should_move && old_dir != new_dir {
            txn.move_directory(old_dir.to_path_buf(), new_dir.clone())?;
        }

        Ok(())
    })();

    // Handle errors
    if let Err(e) = result {
        eprintln!("{} {}", "Error:".red().bold(), e);
        return Err(e);
    }

    // Commit all staged operations
    if let Err(e) = txn.commit() {
        eprintln!("{} {}", "Error during commit:".red().bold(), e);

        // Try to rollback
        if !args.dry_run && !txn.is_empty() {
            eprintln!("{}", "Attempting to rollback changes...".yellow().bold());
            match txn.rollback() {
                Ok(_) => eprintln!("{}", "✓ Rollback successful.".green()),
                Err(rollback_err) => {
                    eprintln!("{} {}", "✗ Rollback failed:".red().bold(), rollback_err);
                }
            }
        }

        return Err(e);
    }

    // Verify the workspace is still valid
    if should_move {
        log::info!("Verifying workspace structure...");
        let verify_result = std::process::Command::new("cargo")
            .arg("metadata")
            .arg("--format-version=1")
            .arg("--no-deps")
            .current_dir(metadata.workspace_root.as_std_path())
            .output();

        match verify_result {
            Ok(output) if output.status.success() => {
                log::debug!("Workspace verification passed");
            }
            Ok(output) => {
                log::error!("Workspace verification failed:");
                log::error!("{}", String::from_utf8_lossy(&output.stderr));
                log::warn!("The rename completed but the workspace may have issues.");
            }
            Err(e) => {
                log::warn!("Could not verify workspace: {}", e);
            }
        }
    }

    // Print summary (can still access txn)
    txn.print_summary(
        &args.old_name,
        &args.new_name,
        metadata.workspace_root.as_std_path(),
    );

    // Success message
    if !args.dry_run {
        println!(
            "{} {} → {}",
            "✓ Successfully renamed".green().bold(),
            args.old_name.yellow(),
            args.new_name.green().bold()
        );
    }

    Ok(())
}
