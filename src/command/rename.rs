use crate::error::{RenameError, Result};
use crate::ops::Transaction;
use crate::ops::{
    update_dependent_manifest, update_package_name, update_source_code, update_workspace_members,
};
use crate::validation::{confirm_operation, preflight_checks};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
pub struct RenameArgs {
    /// Current name of the package to rename
    pub old_name: String,

    /// New name for the package
    pub new_name: String,

    /// Operation mode
    #[command(flatten)]
    pub mode: RenameMode,

    /// Path to Cargo.toml
    #[arg(long, value_name = "PATH")]
    pub manifest_path: Option<PathBuf>,

    /// Preview changes without writing
    #[arg(long, short = 'n')]
    pub dry_run: bool,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Skip git status check
    #[arg(long)]
    pub allow_dirty: bool,

    /// Create git commit after rename
    #[arg(long)]
    pub git_commit: bool,

    /// Show detailed progress information
    #[arg(long, short = 'v')]
    pub verbose: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct RenameMode {
    /// Only rename package name (default)
    #[arg(long, group = "mode")]
    pub name_only: bool,

    /// Only move/rename directory
    #[arg(long, group = "mode")]
    pub path_only: bool,

    /// Rename both name and directory
    #[arg(long, short = 'b', group = "mode")]
    pub both: bool,
}

impl RenameMode {
    pub fn should_rename_package(&self) -> bool {
        self.both || self.name_only || !self.path_only
    }

    pub fn should_move_directory(&self) -> bool {
        self.both || self.path_only
    }
}

pub fn execute(args: RenameArgs) -> Result<()> {
    // Setup logging level based on verbose flag
    if args.verbose {
        log::set_max_level(log::LevelFilter::Debug);
    }

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

    // Find target package
    let target_pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .unwrap(); // Safe: validated in preflight_checks

    let old_manifest_path = target_pkg.manifest_path.as_std_path();
    let old_dir = old_manifest_path.parent().unwrap();

    // Calculate new names and paths
    let new_pkg_name = if args.mode.should_rename_package() {
        &args.new_name
    } else {
        &args.old_name
    };

    let mut new_dir = old_dir.to_path_buf();
    if args.mode.should_move_directory() {
        new_dir.set_file_name(&args.new_name);
    }

    // Create transaction
    let mut txn = Transaction::new(args.dry_run);

    // Show progress header (only in non-dry-run mode)
    if !args.dry_run && args.verbose {
        println!("\n{}", "Executing rename operation...".cyan().bold());
    }

    // Execute all operations with proper error handling
    let result = (|| -> Result<()> {
        // 1. Update all dependent manifests
        if args.verbose {
            println!("\n{} Updating workspace dependencies...", "→".cyan());
        }
        for member_id in &metadata.workspace_members {
            let member = &metadata[member_id];
            update_dependent_manifest(
                member.manifest_path.as_std_path(),
                &args.old_name,
                new_pkg_name,
                &new_dir,
                args.mode.should_move_directory(),
                args.mode.should_rename_package(),
                &mut txn,
            )?;
        }

        // 2. Update target package name
        if args.mode.should_rename_package() {
            if args.verbose {
                println!("\n{} Updating package name...", "→".cyan());
            }
            update_package_name(old_manifest_path, new_pkg_name, &mut txn)?;
        }

        // 3. Update source code references
        if args.mode.should_rename_package() {
            if args.verbose {
                println!("\n{} Updating source code references...", "→".cyan());
            }
            update_source_code(&metadata, &args.old_name, &args.new_name, &mut txn)?;
        }

        // 4. Update workspace members and move directory
        if args.mode.should_move_directory() && old_dir != new_dir {
            if args.verbose {
                println!("\n{} Moving directory...", "→".cyan());
            }
            let root_manifest = metadata.workspace_root.as_std_path().join("Cargo.toml");
            if root_manifest.exists() {
                update_workspace_members(&root_manifest, old_dir, &new_dir, &mut txn)?;
            }
            txn.move_directory(old_dir.to_path_buf(), new_dir.clone())?;
        }

        Ok(())
    })();

    // Handle errors with detailed rollback information
    if let Err(e) = result {
        eprintln!("\n{} {}", "Error:".red().bold(), e);

        if !args.dry_run && !txn.is_empty() {
            eprintln!("\n{}", "Attempting to rollback changes...".yellow().bold());

            match txn.rollback() {
                Ok(_) => {
                    eprintln!("{}", "✓ Rollback successful. No changes were made.".green());
                }
                Err(rollback_err) => {
                    eprintln!("{} Rollback failed: {}", "✗".red().bold(), rollback_err);
                    eprintln!(
                        "\n{}",
                        "WARNING: Your workspace may be in an inconsistent state."
                            .red()
                            .bold()
                    );
                    eprintln!(
                        "{}",
                        "Please check your files manually or restore from git.".yellow()
                    );
                }
            }
        }

        return Err(e);
    }

    // Print detailed summary
    txn.print_summary(
        &args.old_name,
        &args.new_name,
        metadata.workspace_root.as_std_path(),
    );

    // Success message
    if !args.dry_run {
        println!(
            "\n{} Successfully renamed {} to {}",
            "✓".green().bold(),
            args.old_name.yellow(),
            args.new_name.green().bold()
        );

        // Git commit if requested
        if args.git_commit {
            match create_git_commit(&args, &metadata) {
                Ok(_) => {
                    println!("{} Created git commit", "✓".green());
                }
                Err(e) => {
                    eprintln!("{} Failed to create git commit: {}", "⚠".yellow(), e);
                }
            }
        }
    }

    Ok(())
}

fn create_git_commit(args: &RenameArgs, metadata: &cargo_metadata::Metadata) -> Result<()> {
    use std::process::Command;

    let workspace_root = metadata.workspace_root.as_std_path();
    let message = format!("Rename {} to {}", args.old_name, args.new_name);

    // Stage all changes
    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(workspace_root)
        .status()?;

    if !status.success() {
        return Err(RenameError::Other(anyhow::anyhow!("git add failed")));
    }

    // Create commit
    let status = Command::new("git")
        .args(["commit", "-m", &message])
        .current_dir(workspace_root)
        .status()?;

    if !status.success() {
        return Err(RenameError::Other(anyhow::anyhow!("git commit failed")));
    }

    log::info!("Created git commit: {}", message);
    Ok(())
}
