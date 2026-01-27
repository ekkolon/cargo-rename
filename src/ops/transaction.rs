use crate::error::{RenameError, Result};
use colored::Colorize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Operation {
    UpdateFile {
        path: PathBuf,
        original: String,
        new: String,
    },
    MoveDirectory {
        from: PathBuf,
        to: PathBuf,
    },
}

pub struct Transaction {
    operations: Vec<Operation>,
    dry_run: bool,
}

impl Transaction {
    pub fn new(dry_run: bool) -> Self {
        Self {
            operations: Vec::new(),
            dry_run,
        }
    }

    pub fn update_file(&mut self, path: PathBuf, new_content: String) -> Result<()> {
        let original = fs::read_to_string(&path)?;

        if original == new_content {
            return Ok(()); // No change needed
        }

        if self.dry_run {
            log::info!("Would update: {}", path.display());
        } else {
            fs::write(&path, &new_content)?;
            log::debug!("Updated: {}", path.display());
        }

        self.operations.push(Operation::UpdateFile {
            path,
            original,
            new: new_content,
        });

        Ok(())
    }

    pub fn move_directory(&mut self, from: PathBuf, to: PathBuf) -> Result<()> {
        if to.exists() {
            return Err(RenameError::DirectoryExists(to));
        }

        if self.dry_run {
            log::info!("Would move: {} → {}", from.display(), to.display());
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&from, &to)?;
            log::info!("Moved: {} → {}", from.display(), to.display());
        }

        self.operations.push(Operation::MoveDirectory { from, to });
        Ok(())
    }

    pub fn rollback(self) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }

        log::warn!("Rolling back {} operations...", self.operations.len());

        let mut errors = Vec::new();

        for op in self.operations.into_iter().rev() {
            let result = match op {
                Operation::UpdateFile { path, original, .. } => fs::write(&path, original)
                    .map_err(|e| format!("Failed to restore {}: {}", path.display(), e)),
                Operation::MoveDirectory { from, to } => {
                    if to.exists() {
                        fs::rename(&to, &from)
                            .map_err(|e| format!("Failed to move back {}: {}", to.display(), e))
                    } else {
                        Ok(())
                    }
                }
            };

            if let Err(e) = result {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            log::info!("Rollback completed successfully");
            Ok(())
        } else {
            Err(RenameError::RollbackFailed(errors.join("; ")))
        }
    }

    pub fn len(&self) -> usize {
        self.operations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Prints a detailed summary of all operations that were/will be performed
    pub fn print_summary(&self, old_name: &str, new_name: &str, workspace_root: &std::path::Path) {
        if self.operations.is_empty() {
            println!("\n{}", "No changes needed".yellow());
            return;
        }

        // Helper to make paths relative and use forward slashes
        let display_path = |path: &std::path::Path| -> String {
            let relative =
                pathdiff::diff_paths(path, workspace_root).unwrap_or_else(|| path.to_path_buf());
            relative.to_string_lossy().replace('\\', "/")
        };

        // Categorize operations and deduplicate
        let mut package_manifests = std::collections::HashSet::new();
        let mut workspace_manifests = std::collections::HashSet::new();
        let mut source_files = std::collections::HashSet::new();
        let mut doc_files = std::collections::HashSet::new();
        let mut dir_moves = Vec::new();

        for op in &self.operations {
            match op {
                Operation::UpdateFile { path, .. } => {
                    let file_name = path.file_name().unwrap().to_string_lossy();
                    let display = display_path(path);

                    if file_name == "Cargo.toml" {
                        // Check if it's the package being renamed
                        if path
                            .parent()
                            .and_then(|p| p.file_name())
                            .map(|n| {
                                n.to_string_lossy() == old_name || n.to_string_lossy() == new_name
                            })
                            .unwrap_or(false)
                        {
                            package_manifests.insert(display);
                        } else {
                            workspace_manifests.insert(display);
                        }
                    } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                        source_files.insert(display);
                    } else if matches!(
                        path.extension().and_then(|e| e.to_str()),
                        Some("md") | Some("txt")
                    ) {
                        doc_files.insert(display);
                    } else {
                        source_files.insert(display);
                    }
                }
                Operation::MoveDirectory { from, to } => {
                    dir_moves.push((from, to));
                }
            }
        }

        // Convert HashSets to sorted Vecs for consistent output
        let mut package_manifests: Vec<_> = package_manifests.into_iter().collect();
        let mut workspace_manifests: Vec<_> = workspace_manifests.into_iter().collect();
        let mut source_files: Vec<_> = source_files.into_iter().collect();
        let mut doc_files: Vec<_> = doc_files.into_iter().collect();

        package_manifests.sort();
        workspace_manifests.sort();
        source_files.sort();
        doc_files.sort();

        // Print header
        if self.dry_run {
            println!("{}", "DRY RUN - No changes will be made".yellow().bold());
        } else {
            println!("\n{}", "Changes applied:".green().bold());
        }

        // Package manifests
        if !package_manifests.is_empty() {
            println!("\n{} Package manifest", "📦".bold());
            for path in &package_manifests {
                if self.dry_run {
                    println!("   • {}", path.dimmed());
                } else {
                    println!("   {} {}", "✓".green(), path.dimmed());
                }
            }
        }

        // Directory moves
        if !dir_moves.is_empty() {
            println!("\n{} Directory", "📁".bold());
            for (from, to) in &dir_moves {
                let from_name = from.file_name().unwrap().to_string_lossy();
                let to_name = to.file_name().unwrap().to_string_lossy();
                if self.dry_run {
                    println!(
                        "   • {} {} {}",
                        from_name.yellow(),
                        "→".dimmed(),
                        to_name.green()
                    );
                } else {
                    println!(
                        "   {} {} {} {}",
                        "✓".green(),
                        from_name,
                        "→".dimmed(),
                        to_name.green()
                    );
                }
            }
        }

        // Workspace manifests (dependencies)
        if !workspace_manifests.is_empty() {
            println!(
                "\n{} Dependencies ({} file{})",
                "🔗".bold(),
                workspace_manifests.len(),
                if workspace_manifests.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );
            for path in workspace_manifests.iter().take(5) {
                if self.dry_run {
                    println!("   • {}", path.dimmed());
                } else {
                    println!("   {} {}", "✓".green(), path.dimmed());
                }
            }
            if workspace_manifests.len() > 5 {
                println!(
                    "   {} {} more...",
                    if self.dry_run {
                        "•".to_string()
                    } else {
                        "✓".green().to_string()
                    },
                    workspace_manifests.len() - 5
                );
            }
        }

        // Source files
        if !source_files.is_empty() {
            println!(
                "\n{} Source code ({} file{})",
                "📝".bold(),
                source_files.len(),
                if source_files.len() == 1 { "" } else { "s" }
            );
            for path in source_files.iter().take(8) {
                if self.dry_run {
                    println!("   • {}", path.dimmed());
                } else {
                    println!("   {} {}", "✓".green(), path.dimmed());
                }
            }
            if source_files.len() > 8 {
                println!(
                    "   {} {} more...",
                    if self.dry_run {
                        "•".to_string()
                    } else {
                        "✓".green().to_string()
                    },
                    source_files.len() - 8
                );
            }
        }

        // Documentation files
        if !doc_files.is_empty() {
            println!(
                "\n{} Documentation ({} file{})",
                "📄".bold(),
                doc_files.len(),
                if doc_files.len() == 1 { "" } else { "s" }
            );
            for path in doc_files.iter().take(5) {
                if self.dry_run {
                    println!("   • {}", path.dimmed());
                } else {
                    println!("   {} {}", "✓".green(), path.dimmed());
                }
            }
            if doc_files.len() > 5 {
                println!(
                    "   {} {} more...",
                    if self.dry_run {
                        "•".to_string()
                    } else {
                        "✓".green().to_string()
                    },
                    doc_files.len() - 5
                );
            }
        }

        // Summary footer
        // Summary footer
        println!();
        let num_ops = self.operations.len();
        if self.dry_run {
            println!(
                "{} {} will be modified. Run without {} to apply.",
                num_ops.to_string().cyan().bold(),
                if num_ops > 1 { "files" } else { "file" },
                "--dry-run".cyan()
            );
        } else {
            println!(
                "{} Successfully completed {} operations",
                "✓".green().bold(),
                self.operations.len()
            );
        }
    }
}
