use crate::error::{RenameError, Result};
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
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
    CreateDirectory {
        path: PathBuf,
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
            // Create parent directories if needed
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
            return Ok(()); // Nothing to rollback in dry-run
        }

        log::warn!("Rolling back {} operations...", self.operations.len());

        let mut errors = Vec::new();

        // Rollback in reverse order
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
                Operation::CreateDirectory { path } => {
                    if path.exists() {
                        fs::remove_dir_all(&path)
                            .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))
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
        self.operations.len() == 0
    }
}
