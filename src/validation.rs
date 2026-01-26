use crate::command::rename::RenameArgs;
use crate::error::{RenameError, Result};
use cargo_metadata::Metadata;
use colored::Colorize;
use std::path::Path;
use std::process::Command;

pub fn validate_package_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot be empty".to_string(),
        ));
    }

    if !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "must start with a letter".to_string(),
        ));
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "can only contain letters, numbers, hyphens, and underscores".to_string(),
        ));
    }

    let reserved = ["test", "doc", "build", "bench"];
    if reserved.contains(&name) {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "is a reserved package name".to_string(),
        ));
    }

    if name.len() > 64 {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "exceeds maximum length of 64 characters".to_string(),
        ));
    }

    Ok(())
}

pub fn check_git_status(workspace_root: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace_root)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            if !output.stdout.is_empty() {
                return Err(RenameError::DirtyWorkspace);
            }
            Ok(())
        }
        _ => Ok(()), // Not a git repo or git not installed
    }
}

pub fn preflight_checks(args: &RenameArgs, metadata: &Metadata) -> Result<()> {
    validate_package_name(&args.new_name)?;

    let _pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == args.old_name)
        .ok_or_else(|| RenameError::PackageNotFound(args.old_name.clone()))?;

    if !args.skip_git_check
        && let Err(e) = check_git_status(metadata.workspace_root.as_std_path())
    {
        log::warn!("{}", e);
        return Err(e);
    }

    Ok(())
}

pub fn confirm_operation(args: &RenameArgs, metadata: &Metadata) -> Result<bool> {
    if args.yes || args.dry_run {
        return Ok(true);
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
        args.old_name,
        args.new_name
    );

    if args.mode.should_rename_package() {
        println!("  {} Update package name", "✓".green());
    }

    if args.mode.should_move_directory() {
        let _old_dir = pkg.manifest_path.parent().unwrap();
        println!("  {} Move directory to {}", "✓".green(), args.new_name);
    }

    if !dependents.is_empty() {
        println!("  {} Update {} dependent(s)", "✓".green(), dependents.len());
    }

    print!("\n{} [y/N] ", "Continue?".bold());
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut response = String::new();
    std::io::stdin().read_line(&mut response)?;

    Ok(response.trim().eq_ignore_ascii_case("y"))
}
