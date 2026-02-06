//! Workspace-level `Cargo.toml` updates.
//!
//! Handles updates to workspace manifests including:
//! - `[workspace.members]` array
//! - `[workspace.dependencies]` table

use crate::error::Result;
use crate::fs::transaction::Transaction;
use regex::Regex;
use std::fs;
use std::path::Path;

/// Updates workspace-level manifest when a package is renamed or moved.
///
/// This function handles three types of updates:
///
/// 1. **Workspace members**: Updates paths in `[workspace.members]` array
/// 2. **Dependency key**: Renames `old-name = ...` to `new-name = ...` in `[workspace.dependencies]`
/// 3. **Dependency path**: Updates `path = "..."` within the dependency definition
///
/// # Arguments
///
/// - `root_path`: Path to workspace `Cargo.toml`
/// - `old_name`: Current package name
/// - `new_name`: New package name
/// - `old_dir`: Current package directory (absolute path)
/// - `new_dir`: New package directory (absolute path)
/// - `should_update_members`: Whether to update `[workspace.members]`
/// - `path_changed`: Whether the directory path changed
/// - `name_changed`: Whether the package name changed
///
/// # Format Handling
///
/// Handles both single and double quotes:
/// ```toml
/// members = ["path/to/crate", 'other/crate']
/// [workspace.dependencies]
/// my-crate = { path = "crates/my-crate" }
/// ```
///
/// # Path Normalization
///
/// All paths are normalized to forward slashes (`/`) regardless of platform.
///
/// # Errors
///
/// - `Io`: Cannot read/write manifest
/// - `Other`: Path calculation fails
#[allow(clippy::too_many_arguments)]
pub fn update_workspace_manifest(
    root_path: &Path,
    old_name: &str,
    new_name: &str,
    old_dir: &Path,
    new_dir: &Path,
    should_update_members: bool,
    path_changed: bool,
    name_changed: bool,
    txn: &mut Transaction,
) -> Result<()> {
    let mut content = fs::read_to_string(root_path)?;
    let original = content.clone();

    // Update workspace.members
    if should_update_members {
        let root_dir = root_path.parent().unwrap();
        let old_rel = pathdiff::diff_paths(old_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calculate relative path"))?;
        let new_rel = pathdiff::diff_paths(new_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calculate relative path"))?;

        let old_str = old_rel.to_string_lossy().replace('\\', "/");
        let new_str = new_rel.to_string_lossy().replace('\\', "/");

        // Use regex for proper matching (handles special characters in paths)
        // Match both single and double quotes
        let pattern = format!(r#"(["']){}(["'])"#, regex::escape(&old_str));

        if let Ok(re) = Regex::new(&pattern) {
            // Replace while preserving the original quote style
            content = re
                .replace_all(&content, |caps: &regex::Captures| {
                    format!(
                        r#"{quote}{new}{quote}"#,
                        quote = &caps[1], // Preserve original quote style
                        new = new_str
                    )
                })
                .to_string();

            log::info!("Updated workspace.members: {} → {}", old_str, new_str);
        }
    }

    // Update workspace.dependencies key name
    if name_changed {
        let pattern = format!(r"(?m)^(\s*){}\s*=\s*", regex::escape(old_name));
        if let Ok(re) = Regex::new(&pattern) {
            content = re
                .replace_all(&content, format!("${{1}}{} = ", new_name))
                .to_string();
            log::info!(
                "Renamed workspace dependency key: {} → {}",
                old_name,
                new_name
            );
        }
    }

    // Update path within the dependency
    if path_changed {
        let root_dir = root_path.parent().unwrap();
        let old_rel = pathdiff::diff_paths(old_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calculate relative path"))?;
        let new_rel = pathdiff::diff_paths(new_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calculate relative path"))?;

        let old_path = old_rel.to_string_lossy().replace('\\', "/");
        let new_path = new_rel.to_string_lossy().replace('\\', "/");

        // Match: path = "..." or path = '...'
        let pattern = format!(r#"(\bpath\s*=\s*)(["']){}(["'])"#, regex::escape(&old_path));

        if let Ok(re) = Regex::new(&pattern)
            && re.is_match(&content)
        {
            content = re
                .replace_all(&content, |caps: &regex::Captures| {
                    format!(
                        r#"{prefix}{quote}{new}{quote}"#,
                        prefix = &caps[1],
                        quote = &caps[2],
                        new = new_path
                    )
                })
                .to_string();

            log::info!(
                "Updated workspace dependency path: {} → {}",
                old_path,
                new_path
            );
        }
    }

    if content != original {
        txn.update_file(root_path.to_path_buf(), content)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_update_workspace_members() {
        let temp = TempDir::new().unwrap();
        let workspace_toml = temp.path().join("Cargo.toml");

        let input = r#"[workspace]
members = ["crates/old-crate", "crates/other"]
"#;
        fs::write(&workspace_toml, input).unwrap();

        let old_dir = temp.path().join("crates/old-crate");
        let new_dir = temp.path().join("crates/new-crate");

        let mut txn = Transaction::new(false);
        update_workspace_manifest(
            &workspace_toml,
            "old-crate",
            "new-crate",
            &old_dir,
            &new_dir,
            true, // update members
            true, // path changed
            true, // name changed
            &mut txn,
        )
        .unwrap();
        txn.commit().unwrap();

        let result = fs::read_to_string(&workspace_toml).unwrap();
        println!("Result:\n{}", result); // Debug output
        assert!(result.contains(r#""crates/new-crate""#));
        assert!(!result.contains("old-crate"));
    }

    #[test]
    fn test_update_workspace_members_single_quotes() {
        let temp = TempDir::new().unwrap();
        let workspace_toml = temp.path().join("Cargo.toml");

        let input = r#"[workspace]
members = ['crates/old-crate', 'crates/other']
"#;
        fs::write(&workspace_toml, input).unwrap();

        let old_dir = temp.path().join("crates/old-crate");
        let new_dir = temp.path().join("crates/new-crate");

        let mut txn = Transaction::new(false);
        update_workspace_manifest(
            &workspace_toml,
            "old-crate",
            "new-crate",
            &old_dir,
            &new_dir,
            true,
            true,
            true,
            &mut txn,
        )
        .unwrap();
        txn.commit().unwrap();

        let result = fs::read_to_string(&workspace_toml).unwrap();
        // Should preserve single quotes
        assert!(result.contains(r#"'crates/new-crate'"#));
    }

    #[test]
    fn test_update_workspace_dependencies() {
        let temp = TempDir::new().unwrap();
        let workspace_toml = temp.path().join("Cargo.toml");

        let input = r#"[workspace.dependencies]
old-crate = { path = "crates/old-crate" }
"#;
        fs::write(&workspace_toml, input).unwrap();

        let old_dir = temp.path().join("crates/old-crate");
        let new_dir = temp.path().join("crates/new-crate");

        let mut txn = Transaction::new(false);
        update_workspace_manifest(
            &workspace_toml,
            "old-crate",
            "new-crate",
            &old_dir,
            &new_dir,
            false, // don't update members
            true,  // path changed
            true,  // name changed
            &mut txn,
        )
        .unwrap();
        txn.commit().unwrap();

        let result = fs::read_to_string(&workspace_toml).unwrap();
        assert!(result.contains("new-crate = { path = \"crates/new-crate\" }"));
    }

    #[test]
    fn test_preserves_quote_style() {
        let temp = TempDir::new().unwrap();
        let workspace_toml = temp.path().join("Cargo.toml");

        // Mix of quote styles
        let input = r#"[workspace]
members = ["crates/old-crate", 'crates/other']
"#;
        fs::write(&workspace_toml, input).unwrap();

        let old_dir = temp.path().join("crates/old-crate");
        let new_dir = temp.path().join("crates/new-crate");

        let mut txn = Transaction::new(false);
        update_workspace_manifest(
            &workspace_toml,
            "old-crate",
            "new-crate",
            &old_dir,
            &new_dir,
            true,
            true,
            true,
            &mut txn,
        )
        .unwrap();
        txn.commit().unwrap();

        let result = fs::read_to_string(&workspace_toml).unwrap();

        // Should preserve double quotes for first, single for second
        assert!(result.contains(r#""crates/new-crate""#));
        assert!(result.contains(r#"'crates/other'"#));
    }

    #[test]
    fn test_no_changes_if_no_match() {
        let temp = TempDir::new().unwrap();
        let workspace_toml = temp.path().join("Cargo.toml");

        let input = r#"[workspace]
members = ["crates/different"]
"#;
        fs::write(&workspace_toml, input).unwrap();

        let old_dir = temp.path().join("crates/old-crate");
        let new_dir = temp.path().join("crates/new-crate");

        let mut txn = Transaction::new(false);
        update_workspace_manifest(
            &workspace_toml,
            "old-crate",
            "new-crate",
            &old_dir,
            &new_dir,
            true,
            true,
            true,
            &mut txn,
        )
        .unwrap();

        // Should not stage any changes if no match
        assert_eq!(txn.len(), 0);
    }
}
