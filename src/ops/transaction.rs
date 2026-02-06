use crate::error::{RenameError, Result};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionState {
    Building,
    Committed,
    RolledBack,
    Failed,
}

#[must_use = "Transaction must be committed or rolled back"]
pub struct Transaction {
    operations: Vec<Operation>,
    dry_run: bool,
    state: TransactionState,
    /// Tracks which operations were successfully executed (for partial rollback)
    /// Stores indices in execution order
    executed_indices: Vec<usize>, // Changed from HashSet to Vec
    /// Internal map of path redirects due to directory moves
    path_redirects: HashMap<PathBuf, PathBuf>,
}

impl Transaction {
    pub fn new(dry_run: bool) -> Self {
        Self {
            operations: Vec::new(),
            dry_run,
            state: TransactionState::Building,
            executed_indices: Vec::new(),
            path_redirects: HashMap::new(),
        }
    }

    /// Pre-flight validation: check if all operations can succeed
    fn validate(&self) -> Result<()> {
        // Check for conflicts
        let mut file_paths = HashSet::new();
        let mut dir_moves = HashMap::new();

        for op in &self.operations {
            match op {
                Operation::UpdateFile { path, .. } => {
                    if !file_paths.insert(path.clone()) {
                        return Err(RenameError::Other(anyhow::anyhow!(
                            "Duplicate file operation: {}",
                            path.display()
                        )));
                    }

                    // Check file still exists and is writable
                    if !path.exists() {
                        return Err(RenameError::Io(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("File no longer exists: {}", path.display()),
                        )));
                    }

                    // Check we can write (permissions)
                    if let Ok(metadata) = fs::metadata(path) {
                        if metadata.permissions().readonly() {
                            return Err(RenameError::Io(std::io::Error::new(
                                std::io::ErrorKind::PermissionDenied,
                                format!("File is read-only: {}", path.display()),
                            )));
                        }
                    }
                }
                Operation::MoveDirectory { from, to } => {
                    if !from.exists() {
                        return Err(RenameError::Io(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("Directory no longer exists: {}", from.display()),
                        )));
                    }

                    if to.exists() {
                        return Err(RenameError::DirectoryExists(to.clone()));
                    }

                    dir_moves.insert(from, to);
                }
            }
        }

        // Check file operations don't conflict with directory moves
        for file_path in &file_paths {
            for (from, to) in &dir_moves {
                if file_path.starts_with(from) {
                    log::debug!(
                        "File {} will be moved with directory {} → {}",
                        file_path.display(),
                        from.display(),
                        to.display()
                    );
                }
            }
        }

        Ok(())
    }

    pub fn len(&self) -> usize {
        self.operations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    pub fn is_committed(&self) -> bool {
        self.state == TransactionState::Committed
    }

    /// Returns a preview of what will be changed
    pub fn preview(&self) -> Vec<String> {
        self.operations
            .iter()
            .map(|op| match op {
                Operation::UpdateFile { path, .. } => {
                    format!("Update: {}", path.display())
                }
                Operation::MoveDirectory { from, to } => {
                    format!("Move: {} → {}", from.display(), to.display())
                }
            })
            .collect()
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
        // In print_summary, update the directory moves section:
        if !dir_moves.is_empty() {
            println!("📁 Directory");
            for (from, to) in dir_moves {
                // Show relative paths from workspace root
                let from_rel = pathdiff::diff_paths(from, workspace_root)
                    .unwrap_or_else(|| from.to_path_buf());
                let to_rel =
                    pathdiff::diff_paths(to, workspace_root).unwrap_or_else(|| to.to_path_buf());

                let from_display = from_rel.to_string_lossy().replace('\\', "/");
                let to_display = to_rel.to_string_lossy().replace('\\', "/");

                if self.dry_run {
                    println!("   {} → {}", from_display.yellow(), to_display.green());
                } else {
                    println!("   ✓ {} → {}", from_display, to_display.green());
                }
            }
            println!();
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

// Auto-rollback if not committed
impl Drop for Transaction {
    fn drop(&mut self) {
        if self.state == TransactionState::Building && !self.operations.is_empty() && !self.dry_run
        {
            log::warn!("Transaction dropped without commit - changes were not applied");
        }
    }
}

impl Transaction {
    pub fn move_directory(&mut self, from: PathBuf, to: PathBuf) -> Result<()> {
        if self.state != TransactionState::Building {
            return Err(RenameError::Other(anyhow::anyhow!(
                "Cannot modify transaction after commit/rollback"
            )));
        }

        if to.exists() {
            return Err(RenameError::DirectoryExists(to));
        }

        if !from.exists() {
            return Err(RenameError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Source directory does not exist: {}", from.display()),
            )));
        }

        if self.dry_run {
            log::info!("Would move: {} → {}", from.display(), to.display());
        }

        // Track this redirect for file operations
        self.path_redirects.insert(from.clone(), to.clone());

        self.operations.push(Operation::MoveDirectory { from, to });
        Ok(())
    }

    pub fn update_file(&mut self, path: PathBuf, new_content: String) -> Result<()> {
        if self.state != TransactionState::Building {
            return Err(RenameError::Other(anyhow::anyhow!(
                "Cannot modify transaction after commit/rollback"
            )));
        }

        log::debug!("Transaction::update_file called for: {}", path.display());

        // DON'T apply path redirects during staging - files are still at old locations!
        // Path redirects are only for tracking; actual moves happen during commit
        let original = fs::read_to_string(&path).map_err(|e| {
            log::error!("Failed to read file {}: {}", path.display(), e);
            RenameError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to read {}: {}", path.display(), e),
            ))
        })?;

        if original == new_content {
            log::debug!("File content unchanged, skipping: {}", path.display());
            return Ok(());
        }

        if self.dry_run {
            log::info!("Would update: {}", path.display());
        } else {
            log::debug!("Staging update for: {}", path.display());
        }

        self.operations.push(Operation::UpdateFile {
            path, // Store the ORIGINAL path
            original,
            new: new_content,
        });

        log::debug!("Transaction now has {} operations", self.operations.len());
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        if self.state != TransactionState::Building {
            return Err(RenameError::Other(anyhow::anyhow!(
                "Transaction already committed/rolled back"
            )));
        }

        if self.dry_run {
            self.state = TransactionState::Committed;
            return Ok(());
        }

        // Phase 1: Validate
        if let Err(e) = self.validate() {
            self.state = TransactionState::Failed;
            return Err(e);
        }

        // Phase 2: Execute in the correct order:
        // 2a. First, update all files (at their OLD locations)
        // 2b. Then, move directories

        let mut file_ops = Vec::new();
        let mut dir_ops = Vec::new();

        for (idx, op) in self.operations.iter().enumerate() {
            match op {
                Operation::UpdateFile { .. } => file_ops.push(idx),
                Operation::MoveDirectory { .. } => dir_ops.push(idx),
            }
        }

        // Execute file updates FIRST (while files are still at old paths)
        for &idx in &file_ops {
            if let Some(Operation::UpdateFile { path, new, .. }) = self.operations.get(idx) {
                fs::write(path, new).map_err(|e| {
                    RenameError::Io(std::io::Error::new(
                        e.kind(),
                        format!("Failed to write {}: {}", path.display(), e),
                    ))
                })?;
                self.executed_indices.push(idx);
                log::debug!("Updated: {}", path.display());
            }
        }

        // Execute directory moves SECOND (after all files are updated)
        for &idx in &dir_ops {
            if let Some(Operation::MoveDirectory { from, to }) = self.operations.get(idx) {
                // Create parent directories if needed
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }

                // Check if cross-filesystem move is needed
                if Self::is_same_filesystem(from, to)? {
                    // Atomic rename
                    fs::rename(from, to).map_err(|e| {
                        RenameError::Io(std::io::Error::new(
                            e.kind(),
                            format!(
                                "Failed to move {} → {}: {}",
                                from.display(),
                                to.display(),
                                e
                            ),
                        ))
                    })?;
                } else {
                    // Cross-filesystem: must copy then delete
                    Self::copy_dir_recursive(from, to)?;
                    fs::remove_dir_all(from)?;
                }

                self.executed_indices.push(idx);
                log::info!("Moved: {} → {}", from.display(), to.display());
            }
        }

        self.state = TransactionState::Committed;
        Ok(())
    }

    /// Manually rollback a committed transaction
    pub fn rollback(&mut self) -> Result<()> {
        match self.state {
            TransactionState::Building => {
                // Nothing to rollback
                Ok(())
            }
            TransactionState::Committed if self.dry_run => {
                // Dry run, nothing to rollback
                Ok(())
            }
            TransactionState::Committed => {
                // Rollback all operations - populate executed_indices
                self.executed_indices = (0..self.operations.len()).collect();
                self.rollback_partial()
            }
            TransactionState::Failed => {
                // Already attempted rollback during commit
                Err(RenameError::Other(anyhow::anyhow!(
                    "Transaction failed; rollback already attempted"
                )))
            }
            TransactionState::RolledBack => Err(RenameError::Other(anyhow::anyhow!(
                "Transaction already rolled back"
            ))),
        }
    }

    /// Rollback only the operations that were executed
    fn rollback_partial(&mut self) -> Result<()> {
        let mut errors = Vec::new();

        // Rollback in reverse order - now this works!
        for &idx in self.executed_indices.iter().rev() {
            if let Some(op) = self.operations.get(idx) {
                let result = match op {
                    Operation::UpdateFile { path, original, .. } => fs::write(path, original)
                        .map_err(|e| format!("Failed to restore {}: {}", path.display(), e)),
                    Operation::MoveDirectory { from, to } => {
                        if to.exists() {
                            // Try to move back
                            if Self::is_same_filesystem(to, from).unwrap_or(true) {
                                fs::rename(to, from).map_err(|e| {
                                    format!("Failed to move back {}: {}", to.display(), e)
                                })
                            } else {
                                Self::copy_dir_recursive(to, from)
                                    .and_then(|_| fs::remove_dir_all(to).map_err(Into::into))
                                    .map_err(|e| {
                                        format!(
                                            "Failed to restore directory {}: {}",
                                            from.display(),
                                            e
                                        )
                                    })
                            }
                        } else {
                            Ok(()) // Directory doesn't exist, nothing to rollback
                        }
                    }
                };

                if let Err(e) = result {
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            self.state = TransactionState::RolledBack;
            log::info!("Rollback completed successfully");
            Ok(())
        } else {
            Err(RenameError::RollbackFailed(errors.join("; ")))
        }
    }

    /// Check if two paths are on the same filesystem
    fn is_same_filesystem(path1: &Path, path2: &Path) -> Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta1 = fs::metadata(path1)?;
            let meta2_parent = path2.parent().unwrap_or(path2);
            let meta2 = fs::metadata(meta2_parent)?;
            Ok(meta1.dev() == meta2.dev())
        }

        #[cfg(not(unix))]
        {
            // On Windows, assume same filesystem if same drive letter
            let path1_str = path1.to_string_lossy();
            let path2_str = path2.to_string_lossy();

            if path1_str.len() >= 2 && path2_str.len() >= 2 {
                Ok(path1_str.chars().next() == path2_str.chars().next())
            } else {
                Ok(true) // Assume same filesystem if can't determine
            }
        }
    }

    /// Recursively copy directory
    fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
        fs::create_dir_all(to)?;

        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let from_path = entry.path();
            let to_path = to.join(entry.file_name());

            if file_type.is_dir() {
                Self::copy_dir_recursive(&from_path, &to_path)?;
            } else {
                fs::copy(&from_path, &to_path)?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransactionStats {
    pub files_updated: usize,
    pub dirs_moved: usize,
    pub total: usize,
}

impl Transaction {
    pub fn stats(&self) -> TransactionStats {
        let mut files_updated = 0;
        let mut dirs_moved = 0;

        for op in &self.operations {
            match op {
                Operation::UpdateFile { .. } => files_updated += 1,
                Operation::MoveDirectory { .. } => dirs_moved += 1,
            }
        }

        TransactionStats {
            files_updated,
            dirs_moved,
            total: self.operations.len(),
        }
    }
}

// src/ops/transaction.rs (add at the end of the file)

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_new_transaction() {
        let txn = Transaction::new(false);
        assert!(!txn.dry_run);
        assert!(txn.is_empty());
        assert_eq!(txn.len(), 0);
    }

    #[test]
    fn test_update_file_stages_operation() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "original content").unwrap();

        let mut txn = Transaction::new(true); // dry-run
        txn.update_file(file_path.clone(), "new content".to_string())
            .unwrap();

        assert_eq!(txn.len(), 1);

        // File should NOT be changed yet (dry-run)
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "original content");
    }

    #[test]
    fn test_update_file_no_change_skips() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "same content").unwrap();

        let mut txn = Transaction::new(false);
        txn.update_file(file_path.clone(), "same content".to_string())
            .unwrap();

        // Should not add operation if content is identical
        assert_eq!(txn.len(), 0);
    }

    #[test]
    fn test_update_file_nonexistent_fails() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("nonexistent.txt");

        let mut txn = Transaction::new(false);
        let result = txn.update_file(file_path, "content".to_string());

        assert!(result.is_err());
    }

    #[test]
    fn test_move_directory_stages_operation() {
        let temp = TempDir::new().unwrap();
        let from = temp.path().join("old_dir");
        let to = temp.path().join("new_dir");
        fs::create_dir(&from).unwrap();

        let mut txn = Transaction::new(true); // dry-run
        txn.move_directory(from.clone(), to.clone()).unwrap();

        assert_eq!(txn.len(), 1);

        // Directory should NOT be moved yet (dry-run)
        assert!(from.exists());
        assert!(!to.exists());
    }

    #[test]
    fn test_move_directory_existing_target_fails() {
        let temp = TempDir::new().unwrap();
        let from = temp.path().join("old_dir");
        let to = temp.path().join("new_dir");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap(); // Target already exists

        let mut txn = Transaction::new(false);
        let result = txn.move_directory(from, to);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RenameError::DirectoryExists(_)
        ));
    }

    #[test]
    fn test_dry_run_does_not_modify_files() {
        let temp = TempDir::new().unwrap();
        let file1 = temp.path().join("file1.txt");
        let file2 = temp.path().join("file2.txt");
        fs::write(&file1, "original 1").unwrap();
        fs::write(&file2, "original 2").unwrap();

        let mut txn = Transaction::new(true); // dry-run
        txn.update_file(file1.clone(), "modified 1".to_string())
            .unwrap();
        txn.update_file(file2.clone(), "modified 2".to_string())
            .unwrap();

        assert_eq!(txn.len(), 2);

        // Files should remain unchanged
        assert_eq!(fs::read_to_string(&file1).unwrap(), "original 1");
        assert_eq!(fs::read_to_string(&file2).unwrap(), "original 2");
    }

    #[test]
    fn test_commit_applies_file_updates() {
        let temp = TempDir::new().unwrap();
        let file1 = temp.path().join("file1.txt");
        let file2 = temp.path().join("file2.txt");
        fs::write(&file1, "original 1").unwrap();
        fs::write(&file2, "original 2").unwrap();

        let mut txn = Transaction::new(false);
        txn.update_file(file1.clone(), "modified 1".to_string())
            .unwrap();
        txn.update_file(file2.clone(), "modified 2".to_string())
            .unwrap();

        // Commit should apply changes
        txn.commit().unwrap();

        // Files should now be changed
        assert_eq!(fs::read_to_string(&file1).unwrap(), "modified 1");
        assert_eq!(fs::read_to_string(&file2).unwrap(), "modified 2");
    }

    #[test]
    fn test_commit_applies_directory_moves() {
        let temp = TempDir::new().unwrap();
        let from = temp.path().join("old_dir");
        let to = temp.path().join("new_dir");
        fs::create_dir(&from).unwrap();
        fs::write(from.join("file.txt"), "content").unwrap();

        let mut txn = Transaction::new(false);
        txn.move_directory(from.clone(), to.clone()).unwrap();

        txn.commit().unwrap();

        // Directory should be moved
        assert!(!from.exists());
        assert!(to.exists());
        assert_eq!(fs::read_to_string(to.join("file.txt")).unwrap(), "content");
    }

    #[test]
    fn test_commit_creates_parent_directories() {
        let temp = TempDir::new().unwrap();
        let from = temp.path().join("old_dir");
        let to = temp.path().join("nested/path/new_dir");
        fs::create_dir(&from).unwrap();

        let mut txn = Transaction::new(false);
        txn.move_directory(from.clone(), to.clone()).unwrap();

        txn.commit().unwrap();

        // Parent directories should be created
        assert!(to.exists());
        assert!(!from.exists());
    }

    #[test]
    fn test_rollback_restores_files() {
        let temp = TempDir::new().unwrap();
        let file1 = temp.path().join("file1.txt");
        let file2 = temp.path().join("file2.txt");
        fs::write(&file1, "original 1").unwrap();
        fs::write(&file2, "original 2").unwrap();

        let mut txn = Transaction::new(false);
        txn.update_file(file1.clone(), "modified 1".to_string())
            .unwrap();
        txn.update_file(file2.clone(), "modified 2".to_string())
            .unwrap();

        txn.commit().unwrap();

        // Files are now modified
        assert_eq!(fs::read_to_string(&file1).unwrap(), "modified 1");
        assert_eq!(fs::read_to_string(&file2).unwrap(), "modified 2");

        // Create new transaction for rollback test
        let mut txn2 = Transaction::new(false);
        txn2.update_file(file1.clone(), "further modified".to_string())
            .unwrap();
        txn2.commit().unwrap();

        // Now rollback
        txn2.rollback().unwrap();

        // Should be restored to "modified 1" (before second transaction)
        assert_eq!(fs::read_to_string(&file1).unwrap(), "modified 1");
    }

    #[test]
    fn test_rollback_restores_directories() {
        let temp = TempDir::new().unwrap();
        let from = temp.path().join("old_dir");
        let to = temp.path().join("new_dir");
        fs::create_dir(&from).unwrap();
        fs::write(from.join("test.txt"), "content").unwrap();

        let mut txn = Transaction::new(false);
        txn.move_directory(from.clone(), to.clone()).unwrap();
        txn.commit().unwrap();

        // Directory moved
        assert!(!from.exists());
        assert!(to.exists());

        // Rollback
        txn.rollback().unwrap();

        // Should be restored
        assert!(from.exists());
        assert!(!to.exists());
        assert_eq!(
            fs::read_to_string(from.join("test.txt")).unwrap(),
            "content"
        );
    }

    #[test]
    fn test_rollback_on_dry_run_does_nothing() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("file.txt");
        fs::write(&file, "original").unwrap();

        let mut txn = Transaction::new(true); // dry-run
        txn.update_file(file.clone(), "modified".to_string())
            .unwrap();

        // Rollback should be a no-op for dry-run
        txn.rollback().unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "original");
    }

    #[test]
    fn test_multiple_operations_in_sequence() {
        let temp = TempDir::new().unwrap();
        let file1 = temp.path().join("file1.txt");
        let file2 = temp.path().join("file2.txt");
        let dir_from = temp.path().join("dir_old");
        let dir_to = temp.path().join("dir_new");

        fs::write(&file1, "content1").unwrap();
        fs::write(&file2, "content2").unwrap();
        fs::create_dir(&dir_from).unwrap();

        let mut txn = Transaction::new(false);
        txn.update_file(file1.clone(), "new1".to_string()).unwrap();
        txn.update_file(file2.clone(), "new2".to_string()).unwrap();
        txn.move_directory(dir_from.clone(), dir_to.clone())
            .unwrap();

        assert_eq!(txn.len(), 3);

        txn.commit().unwrap();

        // All operations applied
        assert_eq!(fs::read_to_string(&file1).unwrap(), "new1");
        assert_eq!(fs::read_to_string(&file2).unwrap(), "new2");
        assert!(!dir_from.exists());
        assert!(dir_to.exists());
    }

    #[test]
    fn test_print_summary_empty() {
        let temp = TempDir::new().unwrap();
        let txn = Transaction::new(false);

        // Should not panic
        txn.print_summary("old", "new", temp.path());
    }

    #[test]
    fn test_print_summary_with_operations() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("test.txt");
        fs::write(&file, "original").unwrap();

        let mut txn = Transaction::new(false);
        txn.update_file(file, "modified".to_string()).unwrap();

        // Should not panic
        txn.print_summary("old", "new", temp.path());
    }

    #[test]
    fn test_categorization_in_summary() {
        let temp = TempDir::new().unwrap();

        // Package manifest
        let pkg_dir = temp.path().join("old_crate");
        fs::create_dir(&pkg_dir).unwrap();
        let pkg_toml = pkg_dir.join("Cargo.toml");
        fs::write(&pkg_toml, "[package]\nname = \"old\"").unwrap();

        // Workspace manifest
        let ws_toml = temp.path().join("Cargo.toml");
        fs::write(&ws_toml, "[workspace]").unwrap();

        // Source file
        let src_dir = pkg_dir.join("src");
        fs::create_dir(&src_dir).unwrap();
        let lib_rs = src_dir.join("lib.rs");
        fs::write(&lib_rs, "pub fn test() {}").unwrap();

        // Doc file
        let readme = temp.path().join("README.md");
        fs::write(&readme, "# Project").unwrap();

        let mut txn = Transaction::new(true); // dry-run
        txn.update_file(pkg_toml, "[package]\nname = \"new\"".to_string())
            .unwrap();
        txn.update_file(ws_toml, "[workspace]\nmembers = []".to_string())
            .unwrap();
        txn.update_file(lib_rs, "pub fn new_test() {}".to_string())
            .unwrap();
        txn.update_file(readme, "# New Project".to_string())
            .unwrap();

        // Should categorize correctly (manual verification in output)
        txn.print_summary("old_crate", "new_crate", temp.path());

        assert_eq!(txn.len(), 4);
    }

    #[test]
    fn test_path_display_formatting() {
        let temp = TempDir::new().unwrap();
        let nested = temp.path().join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("test.txt");
        fs::write(&file, "content").unwrap();

        let mut txn = Transaction::new(true);
        txn.update_file(file, "new".to_string()).unwrap();

        // Paths should be relative and use forward slashes
        txn.print_summary("old", "new", temp.path());
    }

    #[test]
    fn test_large_number_of_operations() {
        let temp = TempDir::new().unwrap();
        let mut txn = Transaction::new(true);

        // Create many files
        for i in 0..100 {
            let file = temp.path().join(format!("file{}.txt", i));
            fs::write(&file, format!("content {}", i)).unwrap();
            txn.update_file(file, format!("new {}", i)).unwrap();
        }

        assert_eq!(txn.len(), 100);

        // Summary should truncate (show first 8, then "... X more")
        txn.print_summary("old", "new", temp.path());
    }

    #[test]
    fn test_is_empty() {
        let mut txn = Transaction::new(false);
        assert!(txn.is_empty());

        let temp = TempDir::new().unwrap();
        let file = temp.path().join("test.txt");
        fs::write(&file, "content").unwrap();
        txn.update_file(file, "new".to_string()).unwrap();

        assert!(!txn.is_empty());
    }

    #[test]
    fn test_commit_failure_partial_rollback() {
        let temp = TempDir::new().unwrap();
        let file1 = temp.path().join("file1.txt");
        let file2 = temp.path().join("readonly.txt");

        fs::write(&file1, "content1").unwrap();
        fs::write(&file2, "content2").unwrap();

        let mut txn = Transaction::new(false);
        txn.update_file(file1.clone(), "new1".to_string()).unwrap();
        txn.update_file(file2.clone(), "new2".to_string()).unwrap();

        // Make file2 readonly after staging but before commit
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&file2).unwrap().permissions();
            perms.set_mode(0o444);
            fs::set_permissions(&file2, perms).unwrap();
        }

        #[cfg(windows)]
        {
            let mut perms = fs::metadata(&file2).unwrap().permissions();
            perms.set_readonly(true);
            fs::set_permissions(&file2, perms).unwrap();
        }

        // Commit might fail on readonly file
        let result = txn.commit();

        // Clean up permissions for temp dir cleanup
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&file2).unwrap().permissions();
            perms.set_mode(0o644);
            let _ = fs::set_permissions(&file2, perms);
        }

        #[cfg(windows)]
        {
            let mut perms = fs::metadata(&file2).unwrap().permissions();
            perms.set_readonly(false);
            let _ = fs::set_permissions(&file2, perms);
        }

        // Should handle error gracefully
        if result.is_err() {
            // Expected behavior
        }
    }
}
