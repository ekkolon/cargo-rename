use crate::error::Result;
use crate::ops::{
    Transaction, update_dependent_manifest, update_package_name, update_source_code,
    update_workspace_members,
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
    #[arg(long, short = 'd')]
    pub dry_run: bool,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Skip git status check
    #[arg(long)]
    pub skip_git_check: bool,

    /// Create git commit after rename
    #[arg(long)]
    pub git_commit: bool,
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
    let mut cmd = MetadataCommand::new();
    if let Some(path) = &args.manifest_path {
        cmd.manifest_path(path);
    }
    let metadata = cmd.exec()?;

    preflight_checks(&args, &metadata)?;

    if !confirm_operation(&args, &metadata)? {
        return Err(crate::error::RenameError::Cancelled);
    }

    let target_pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .unwrap();

    let old_manifest_path = target_pkg.manifest_path.as_std_path();
    let old_dir = old_manifest_path.parent().unwrap();

    let new_pkg_name = if args.mode.should_rename_package() {
        &args.new_name
    } else {
        &args.old_name
    };

    let mut new_dir = old_dir.to_path_buf();
    if args.mode.should_move_directory() {
        new_dir.set_file_name(&args.new_name);
    }

    let mut txn = Transaction::new(args.dry_run);

    let result = (|| -> Result<()> {
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

        if args.mode.should_rename_package() {
            update_package_name(old_manifest_path, new_pkg_name, &mut txn)?;
        }

        if args.mode.should_rename_package() {
            update_source_code(&metadata, &args.old_name, &args.new_name, &mut txn)?;
        }

        if args.mode.should_move_directory() && old_dir != new_dir {
            let root_manifest = metadata.workspace_root.as_std_path().join("Cargo.toml");
            if root_manifest.exists() {
                update_workspace_members(&root_manifest, old_dir, &new_dir, &mut txn)?;
            }
            txn.move_directory(old_dir.to_path_buf(), new_dir.clone())?;
        }

        Ok(())
    })();

    if let Err(e) = result {
        if !args.dry_run && !txn.is_empty() {
            eprintln!("{}", "Operation failed, rolling back...".red().bold());
            txn.rollback()?;
        }
        return Err(e);
    }

    if args.dry_run {
        println!(
            "\n{} {} operations planned",
            "Dry run:".yellow().bold(),
            txn.len()
        );
    } else {
        println!(
            "\n{} Renamed {} to {}",
            "Success:".green().bold(),
            args.old_name,
            args.new_name
        );
    }

    Ok(())
}
