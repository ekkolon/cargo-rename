use anyhow::{Context, Result, bail};
use cargo_metadata::MetadataCommand;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Value};

use crate::cli::RenameArgs;

pub fn execute_rename(args: RenameArgs) -> Result<()> {
    let RenameArgs {
        old_name,
        new_name,
        name_only,
        path_only,
        dry_run,
        manifest_path,
    } = args;

    let mut cmd = MetadataCommand::new();
    if let Some(path) = &manifest_path {
        cmd.manifest_path(path);
    }
    let metadata = cmd.exec()?;

    let target_pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == old_name)
        .ok_or_else(|| anyhow::anyhow!("Package '{}' not found", old_name))?;

    let old_manifest_path = target_pkg.manifest_path.as_std_path();
    let old_dir = old_manifest_path.parent().unwrap();

    let effective_pkg_name = if name_only { &new_name } else { &old_name };
    let mut new_dir = old_dir.to_path_buf();
    if path_only {
        new_dir.set_file_name(&new_name);
    }

    // Track state for rollback
    let mut backups: std::collections::HashMap<PathBuf, String> = std::collections::HashMap::new();

    let result = (|| -> Result<()> {
        // 1. Backup and Update Dependents
        for member_id in &metadata.workspace_members {
            let m = &metadata[member_id];
            let p = m.manifest_path.as_std_path();
            backups.insert(p.to_path_buf(), fs::read_to_string(p)?);
            update_dependent_manifest(
                p,
                &old_name,
                effective_pkg_name,
                &new_dir,
                path_only,
                name_only,
                dry_run,
            )?;
        }

        // 2. Backup and Update Target Name
        if name_only {
            if !backups.contains_key(old_manifest_path) {
                backups.insert(
                    old_manifest_path.to_path_buf(),
                    fs::read_to_string(old_manifest_path)?,
                );
            }
            update_package_name(old_manifest_path, effective_pkg_name, dry_run)?;
            update_project_content(&metadata, &old_name, &new_name, dry_run)?;
        }

        // 3. Workspace Members & Move
        if path_only && old_dir != new_dir {
            let root_toml = metadata.workspace_root.as_std_path().join("Cargo.toml");
            if root_toml.exists() {
                backups.insert(root_toml.clone(), fs::read_to_string(&root_toml)?);
                update_workspace_members(&root_toml, old_dir, &new_dir, dry_run)?;
            }
            perform_move(old_dir, &new_dir, dry_run)?;
        }

        // 4. Verify
        if !dry_run {
            verify_workspace(&metadata.workspace_root.as_std_path().join("Cargo.toml"))?;
        }
        Ok(())
    })();

    // Handle Rollback on failure
    if let Err(e) = result {
        if !dry_run {
            eprintln!("{} {}. Attempting rollback...", "Error:".red().bold(), e);
            for (path, content) in backups {
                let _ = fs::write(path, content);
            }
            if path_only && new_dir.exists() && !old_dir.exists() {
                let _ = fs::rename(&new_dir, old_dir);
            }
        }
        return Err(e);
    }

    println!("{:>12} renaming process", "Finished".green().bold());
    Ok(())
}

fn perform_move(old_dir: &Path, new_dir: &Path, dry_run: bool) -> Result<()> {
    if dry_run {
        println!(
            "{:>12} directory {} -> {}",
            "Pending".blue().bold(),
            old_dir.display(),
            new_dir.display()
        );
        return Ok(());
    }

    if new_dir.exists() {
        bail!("Target directory already exists: {}", new_dir.display());
    }

    // Ensure the parent directory exists (critical for nested moves like crates/new-path)
    if let Some(parent) = new_dir.parent() {
        fs::create_dir_all(parent).context("Failed to create parent directories for move")?;
    }

    fs::rename(old_dir, new_dir).with_context(|| {
        format!(
            "Failed to move {} to {}",
            old_dir.display(),
            new_dir.display()
        )
    })?;

    println!(
        "{:>12} {} to {}",
        "Moved".green().bold(),
        old_dir.display(),
        new_dir.display()
    );
    Ok(())
}

fn verify_workspace(root_manifest: &Path) -> Result<()> {
    println!("{:>12} workspace integrity...", "Verifying".cyan().bold());

    // Check if the manifest actually exists before calling cargo
    if !root_manifest.exists() {
        return Ok(()); // Standalone crate, nothing to verify via workspace root
    }

    let status = std::process::Command::new("cargo")
        .arg("fetch")
        .arg("--manifest-path")
        .arg(root_manifest)
        .status()
        .context("Failed to run cargo fetch to verify workspace")?;

    if !status.success() {
        bail!("Workspace is in an inconsistent state. Please check your Cargo.toml files.");
    }

    Ok(())
}

fn update_dependent_manifest(
    manifest_path: &Path,
    old_name: &str,
    new_name: &str,
    new_dir: &Path,
    path_changed: bool,
    name_changed: bool,
    dry_run: bool,
) -> Result<()> {
    let content = fs::read_to_string(manifest_path)?;
    let mut doc = content.parse::<DocumentMut>()?;
    let mut changed = false;
    let manifest_dir = manifest_path.parent().unwrap();

    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(deps) = doc.get_mut(section).and_then(|i| i.as_table_mut()) {
            // 1. Handle Key Renames and Aliases
            // We collect keys first to avoid borrow checker issues while mutating the table
            let keys: Vec<String> = deps.iter().map(|(k, _)| k.to_string()).collect();

            for key in keys {
                let mut is_target = &key == old_name;
                let mut item = deps.remove(&key).unwrap();

                // Check for 'package' field alias: my-alias = { package = "old_name", ... }
                if let Some(pkg_field) = get_package_field(&item) {
                    if pkg_field == old_name {
                        is_target = true;
                        if name_changed {
                            set_package_field(&mut item, new_name);
                            changed = true;
                        }
                    }
                }

                if is_target {
                    // Update Path if the directory moved
                    if path_changed {
                        if let Some(_) = get_path_field(&item) {
                            let rel_path = pathdiff::diff_paths(new_dir, manifest_dir)
                                .context("Failed to calculate relative path")?;
                            set_path_field(&mut item, &rel_path.to_string_lossy());
                            changed = true;
                        }
                    }

                    // Determine final key name
                    let final_key = if &key == old_name && name_changed {
                        new_name.to_string()
                    } else {
                        key
                    };

                    deps.insert(&final_key, item);
                    if final_key != old_name {
                        changed = true;
                    }
                } else {
                    // Put it back if it wasn't our target
                    deps.insert(&key, item);
                }
            }
        }
    }

    if changed {
        if dry_run {
            println!(
                "{:>12} updates to {}",
                "Pending".blue(),
                manifest_path.display()
            );
        } else {
            fs::write(manifest_path, doc.to_string())?;
        }
    }
    Ok(())
}

// --- TOML abstraction helpers to handle both Table and InlineTable ---

fn get_package_field(item: &Item) -> Option<&str> {
    item.get("package").and_then(|v| v.as_str())
}

fn set_package_field(item: &mut Item, val: &str) {
    if let Some(t) = item.as_table_mut() {
        t.insert("package", Item::Value(val.into()));
    } else if let Some(t) = item.as_inline_table_mut() {
        t.insert("package", val.into());
    }
}

fn get_path_field(item: &Item) -> Option<&str> {
    item.get("path").and_then(|v| v.as_str())
}

fn set_path_field(item: &mut Item, val: &str) {
    if let Some(t) = item.as_table_mut() {
        t.insert("path", Item::Value(val.into()));
    } else if let Some(t) = item.as_inline_table_mut() {
        t.insert("path", val.into());
    }
}

// Helpers for toml_edit Item types
fn update_workspace_members(
    root_path: &Path,
    old_dir: &Path,
    new_dir: &Path,
    dry: bool,
) -> Result<()> {
    let content = fs::read_to_string(root_path)?;
    let mut doc = content.parse::<DocumentMut>()?;
    let mut changed = false;

    if let Some(workspace) = doc.get_mut("workspace").and_then(|w| w.as_table_mut()) {
        if let Some(members) = workspace.get_mut("members").and_then(|m| m.as_array_mut()) {
            let root_dir = root_path.parent().unwrap();

            // Calculate what the old and new strings would look like in the TOML
            let old_rel = pathdiff::diff_paths(old_dir, root_dir)
                .context("Failed to calc old relative path")?;
            let new_rel = pathdiff::diff_paths(new_dir, root_dir)
                .context("Failed to calc new relative path")?;

            let old_str = old_rel.to_string_lossy();
            let new_str = new_rel.to_string_lossy();

            for i in 0..members.len() {
                if let Some(member_path) = members.get(i).and_then(|v| v.as_str()) {
                    if member_path == old_str {
                        members.replace(i, new_str.as_ref());
                        changed = true;
                        break;
                    }
                }
            }
        }
    }

    if changed {
        if dry {
            println!(
                "{:>12} workspace members in {}",
                "Pending".blue(),
                root_path.display()
            );
        } else {
            fs::write(root_path, doc.to_string())?;
        }
    }
    Ok(())
}

fn update_package_name(manifest_path: &Path, new_name: &str, dry_run: bool) -> Result<()> {
    let content = fs::read_to_string(manifest_path)?;
    let mut doc = content.parse::<DocumentMut>()?;

    // Update [package] name = "..."
    doc["package"]["name"] = Item::Value(Value::from(new_name));

    if dry_run {
        println!(
            "{:>12} package name to '{}' in {}",
            "Pending".blue().bold(),
            new_name,
            manifest_path.display()
        );
    } else {
        fs::write(manifest_path, doc.to_string())
            .context("Failed to write updated package name to manifest")?;
    }

    Ok(())
}

fn update_project_content(
    metadata: &cargo_metadata::Metadata,
    old_name: &str,
    new_name: &str,
    dry_run: bool,
) -> Result<()> {
    let old_snake = old_name.replace('-', "_");
    let new_snake = new_name.replace('-', "_");

    // Pass 1: Strict Rust Code Patterns
    // Patterns: 'use old::', 'extern crate old;', 'old::Member'
    let rust_patterns = [
        format!("use {}", regex::escape(&old_snake)),
        format!(r"extern crate {}", regex::escape(&old_snake)),
        format!(r"{}\s*::", regex::escape(&old_snake)),
    ];
    let _rust_re = regex::RegexSet::new(&rust_patterns)?;
    let rust_individual_re: Vec<regex::Regex> = rust_patterns
        .iter()
        .map(|p| regex::Regex::new(p).unwrap())
        .collect();

    for member in metadata.workspace_packages() {
        let pkg_root = member.manifest_path.parent().unwrap();

        for entry in walkdir::WalkDir::new(pkg_root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let ext = path.extension().and_then(|s| s.to_str());

            match ext {
                Some("rs") => {
                    let content = fs::read_to_string(path)?;
                    let mut new_content = content.clone();
                    let mut changed = false;

                    for re in &rust_individual_re {
                        if re.is_match(&new_content) {
                            // We only replace the identifier part of the match
                            // Example: 'use old_name' -> 'use new_name'
                            // We use a capture group to be safer if needed,
                            // but here we know the patterns are specific.
                            let replacement = re.as_str().replace(&old_snake, &new_snake);
                            new_content = re
                                .replace_all(&new_content, replacement.as_str())
                                .to_string();
                            changed = true;
                        }
                    }

                    if changed && !dry_run {
                        fs::write(path, new_content)?;
                    }
                }
                Some("md") | Some("toml") => {
                    // Documentation is less strict; we use the word boundary kebab-case
                    let content = fs::read_to_string(path)?;
                    let re_kebab = regex::Regex::new(&format!(r"\b{}\b", regex::escape(old_name)))?;

                    if re_kebab.is_match(&content) {
                        if !dry_run {
                            let nc = re_kebab.replace_all(&content, new_name);
                            fs::write(path, nc.to_string())?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
