// src/ops/manifest.rs - Complete production-ready implementation

use crate::error::Result;
use crate::ops::transaction::Transaction;
use regex::Regex;
use std::fs;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Value};

/// Updates workspace-level Cargo.toml manifest
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

    // Step 1: Update workspace.members
    if should_update_members {
        let root_dir = root_path.parent().unwrap();
        let old_rel = pathdiff::diff_paths(old_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;
        let new_rel = pathdiff::diff_paths(new_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;

        let old_str = old_rel.to_string_lossy().replace('\\', "/");
        let new_str = new_rel.to_string_lossy().replace('\\', "/");

        // Replace with both quote styles
        let patterns = vec![
            format!(r#""{}""#, regex::escape(&old_str)),
            format!(r#"'{}'"#, regex::escape(&old_str)),
        ];

        for pattern in patterns {
            let replacement = format!(r#""{}""#, new_str);
            content = content.replace(&pattern, &replacement);
        }

        log::info!("Updated workspace.members: {} → {}", old_str, new_str);
    }

    // Step 2: Update workspace.dependencies key name
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

    // Step 3: Update path within the dependency
    if path_changed {
        let root_dir = root_path.parent().unwrap();
        let old_rel = pathdiff::diff_paths(old_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;
        let new_rel = pathdiff::diff_paths(new_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;

        let old_path = old_rel.to_string_lossy().replace('\\', "/");
        let new_path = new_rel.to_string_lossy().replace('\\', "/");

        // Handle both quote styles
        let patterns = vec![
            format!(r#"path\s*=\s*"{}""#, regex::escape(&old_path)),
            format!(r#"path\s*=\s*'{}'"#, regex::escape(&old_path)),
        ];

        for pattern in patterns {
            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(&content) {
                    content = re
                        .replace_all(&content, format!(r#"path = "{}""#, new_path))
                        .to_string();
                    log::info!(
                        "Updated workspace dependency path: {} → {}",
                        old_path,
                        new_path
                    );
                    break;
                }
            }
        }
    }

    if content != original {
        txn.update_file(root_path.to_path_buf(), content)?;
    }

    Ok(())
}

/// Updates package name in a Cargo.toml
pub fn update_package_name(
    manifest_path: &Path,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let content = fs::read_to_string(manifest_path)?;
    let mut doc: DocumentMut = content.parse()?;
    doc["package"]["name"] = Item::Value(Value::from(new_name));
    txn.update_file(manifest_path.to_path_buf(), doc.to_string())?;
    Ok(())
}

/// Production-ready implementation of update_dependent_manifest
/// Handles ALL edge cases including:
/// - Multi-line inline tables
/// - Target-specific dependencies  
/// - Mixed quote styles
/// - Inline comments
/// - Platform-specific path separators
/// - Package renames
/// - Workspace dependencies
pub fn update_dependent_manifest(
    manifest_path: &Path,
    old_name: &str,
    new_name: &str,
    new_dir: &Path,
    path_changed: bool,
    name_changed: bool,
    txn: &mut Transaction,
) -> Result<()> {
    let content = fs::read_to_string(manifest_path)?;
    let original = content.clone();
    let manifest_dir = manifest_path.parent().unwrap();

    if !name_changed && !path_changed {
        return Ok(());
    }

    log::debug!("Updating dependent manifest: {}", manifest_path.display());

    // Calculate new relative path once
    let new_path_str = if path_changed {
        let rel_path = pathdiff::diff_paths(new_dir, manifest_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calculate relative path"))?;
        Some(rel_path.to_string_lossy().replace('\\', "/"))
    } else {
        None
    };

    let mut processor = TomlProcessor::new(&content, old_name, new_name, new_path_str.as_deref());
    let new_content = processor.process(name_changed, path_changed)?;

    if new_content != original {
        txn.update_file(manifest_path.to_path_buf(), new_content)?;
        log::debug!("Updated dependent manifest: {}", manifest_path.display());
    } else {
        log::debug!("No changes needed for: {}", manifest_path.display());
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum DependencySection {
    Dependencies,
    DevDependencies,
    BuildDependencies,
    TargetDependencies(String), // e.g., "cfg(windows)"
}

struct TomlProcessor<'a> {
    lines: Vec<&'a str>,
    old_name: &'a str,
    new_name: &'a str,
    new_path: Option<&'a str>,
    had_trailing_newline: bool, // Add this

    // State tracking
    current_section: Option<DependencySection>,
    in_target_dep: bool,
    in_package_dep: bool,
    brace_depth: i32,
    multiline_table_dep: Option<String>,
}

impl<'a> TomlProcessor<'a> {
    fn new(
        content: &'a str,
        old_name: &'a str,
        new_name: &'a str,
        new_path: Option<&'a str>,
    ) -> Self {
        Self {
            lines: content.lines().collect(),
            old_name,
            new_name,
            new_path,
            had_trailing_newline: content.ends_with('\n'), // Track this
            current_section: None,
            in_target_dep: false,
            in_package_dep: false,
            brace_depth: 0,
            multiline_table_dep: None,
        }
    }

    fn process(&mut self, name_changed: bool, path_changed: bool) -> Result<String> {
        let mut result_lines = Vec::new();

        // Always search for the OLD name in the source
        let search_dep = self.old_name;

        // Clone the lines to avoid borrow checker issues
        let lines_copy: Vec<String> = self.lines.iter().map(|s| s.to_string()).collect();

        for line in &lines_copy {
            let mut modified_line = line.clone();
            let trimmed = line.trim();

            // Track section changes
            self.update_section(trimmed);

            // Handle section headers
            if self.is_section_header(trimmed) {
                if name_changed {
                    modified_line = self.rename_section_header(&line)?;
                }
                self.reset_state();
                result_lines.push(modified_line);
                continue;
            }

            // Handle standalone path lines in multi-line tables
            if self.brace_depth == 0
                && trimmed.starts_with("path")
                && self.is_in_target_context(search_dep)
                && path_changed
            {
                modified_line = self.update_standalone_path(&line)?;
                result_lines.push(modified_line);
                continue;
            }

            // Handle dependency declaration lines - ALWAYS search for old name
            if self.is_dependency_line(trimmed, search_dep) {
                self.start_dependency_tracking(&line, search_dep);

                if name_changed {
                    modified_line = self.rename_dependency_key(&line)?;
                }

                if path_changed {
                    modified_line = self.update_inline_path(&modified_line)?;
                }

                result_lines.push(modified_line);
                continue;
            }

            // Handle continuation of multi-line inline tables
            if self.brace_depth > 0 {
                if path_changed {
                    modified_line = self.update_inline_path(&line)?;
                }
                self.update_brace_depth(&line);
                result_lines.push(modified_line);
                continue;
            }

            // Handle lines with package field
            if name_changed && self.has_package_field(&line) {
                self.start_dependency_tracking(&line, search_dep);
                modified_line = self.rename_package_field(&line)?;

                if path_changed && self.has_path_field(&line) {
                    modified_line = self.update_inline_path(&modified_line)?;
                }

                result_lines.push(modified_line);
                continue;
            }

            // No changes needed
            result_lines.push(modified_line);
        }

        let mut result = result_lines.join("\n");

        // Preserve trailing newline if original had one
        if self.had_trailing_newline && !result.ends_with('\n') {
            result.push('\n');
        }

        Ok(result)
    }

    fn update_section(&mut self, trimmed: &str) {
        if !trimmed.starts_with('[') {
            return;
        }

        // Parse section header
        if let Some(section) = self.parse_section(trimmed) {
            self.current_section = Some(section);
            self.multiline_table_dep = None;

            // Check if it's a dependency-specific section like [dependencies.my-crate]
            if let Some(dep_name) = self.extract_dep_from_section(trimmed) {
                self.multiline_table_dep = Some(dep_name);
            }
        }
    }

    fn parse_section(&self, header: &str) -> Option<DependencySection> {
        // Match [dependencies], [dev-dependencies], [build-dependencies]
        if header.starts_with("[dependencies") {
            return Some(DependencySection::Dependencies);
        }
        if header.starts_with("[dev-dependencies") {
            return Some(DependencySection::DevDependencies);
        }
        if header.starts_with("[build-dependencies") {
            return Some(DependencySection::BuildDependencies);
        }

        // Match [target.'cfg(...)'.dependencies]
        if header.starts_with("[target.") {
            if let Some(target) = self.extract_target_triple(header) {
                return Some(DependencySection::TargetDependencies(target));
            }
        }

        None
    }

    fn extract_target_triple(&self, header: &str) -> Option<String> {
        // Extract the target from [target.'cfg(windows)'.dependencies]
        let pattern = Regex::new(r"\[target\.'([^']+)'\.").ok()?;
        pattern
            .captures(header)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
    }

    fn extract_dep_from_section(&self, header: &str) -> Option<String> {
        // Extract "my-crate" from [dependencies.my-crate]
        let pattern =
            Regex::new(r"\[(?:dependencies|dev-dependencies|build-dependencies)\.([^\]]+)\]")
                .ok()?;
        pattern
            .captures(header)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
    }

    fn is_section_header(&self, trimmed: &str) -> bool {
        trimmed.starts_with('[') && trimmed.ends_with(']')
    }

    fn reset_state(&mut self) {
        self.in_target_dep = false;
        self.in_package_dep = false;
        self.brace_depth = 0;
    }

    fn is_in_target_context(&self, target_dep: &str) -> bool {
        if let Some(ref dep) = self.multiline_table_dep {
            return dep == target_dep;
        }
        self.in_target_dep || self.in_package_dep
    }

    fn is_dependency_line(&self, trimmed: &str, target_dep: &str) -> bool {
        // Check for: target-dep = ...
        // But not inside brackets
        if trimmed.starts_with('[') {
            return false;
        }

        let pattern = format!(r"^{}\s*[.=]", regex::escape(target_dep));
        Regex::new(&pattern)
            .map(|re| re.is_match(trimmed))
            .unwrap_or(false)
    }

    fn start_dependency_tracking(&mut self, line: &str, target_dep: &str) {
        // Check if this is our target dependency
        let key_pattern = format!(r"^\s*{}\s*=\s*\{{", regex::escape(target_dep));
        if let Ok(re) = Regex::new(&key_pattern) {
            if re.is_match(line) {
                self.in_target_dep = true;
                self.in_package_dep = false;
                self.update_brace_depth(line);
                return;
            }
        }

        // Check if this has package = "target_dep"
        let package_pattern = format!(r#"package\s*=\s*["']{}["']"#, regex::escape(target_dep));
        if let Ok(re) = Regex::new(&package_pattern) {
            if re.is_match(line) {
                self.in_package_dep = true;
                self.in_target_dep = false;
                self.update_brace_depth(line);
            }
        }
    }

    fn update_brace_depth(&mut self, line: &str) {
        self.brace_depth += line.matches('{').count() as i32;
        self.brace_depth -= line.matches('}').count() as i32;

        if self.brace_depth == 0 {
            self.in_target_dep = false;
            self.in_package_dep = false;
        }
    }

    fn has_package_field(&self, line: &str) -> bool {
        let pattern = format!(r#"package\s*=\s*["']{}["']"#, regex::escape(self.old_name));
        Regex::new(&pattern)
            .map(|re| re.is_match(line))
            .unwrap_or(false)
    }

    fn has_path_field(&self, line: &str) -> bool {
        Regex::new(r#"\bpath\s*=\s*["']"#)
            .map(|re| re.is_match(line))
            .unwrap_or(false)
    }

    fn rename_section_header(&self, line: &str) -> Result<String> {
        // Rename [dependencies.old-name] to [dependencies.new-name]
        let sections = ["dependencies", "dev-dependencies", "build-dependencies"];

        for section in sections {
            let pattern = format!(
                r"^(\s*\[(?:target\.[^]]+\.)?{}\.){}\]",
                regex::escape(section),
                regex::escape(self.old_name)
            );

            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(line) {
                    return Ok(re
                        .replace(line, format!("${{1}}{}]", self.new_name))
                        .to_string());
                }
            }
        }

        Ok(line.to_string())
    }

    fn rename_dependency_key(&self, line: &str) -> Result<String> {
        // Rename old-name = ... to new-name = ...
        // Handle both: old-name = ... and old-name.workspace = ...

        // Pattern 1: old-name.workspace = true
        let ws_pattern = format!(
            r"^(\s*){}\s*\.\s*workspace\s*=",
            regex::escape(self.old_name)
        );
        if let Ok(re) = Regex::new(&ws_pattern) {
            if re.is_match(line) {
                return Ok(re
                    .replace(line, format!("${{1}}{}.workspace =", self.new_name))
                    .to_string());
            }
        }

        // Pattern 2: old-name = ...
        let key_pattern = format!(r"^(\s*){}\s*=\s*", regex::escape(self.old_name));
        if let Ok(re) = Regex::new(&key_pattern) {
            if re.is_match(line) {
                return Ok(re
                    .replace(line, format!("${{1}}{} = ", self.new_name))
                    .to_string());
            }
        }

        Ok(line.to_string())
    }

    fn rename_package_field(&self, line: &str) -> Result<String> {
        // Double quotes: package = "old-name"
        // Capture: (package = ")old-name(")
        let double_pattern = format!(r#"(\bpackage\s*=\s*"){}(")"#, regex::escape(self.old_name));
        if let Ok(re) = Regex::new(&double_pattern) {
            if re.is_match(line) {
                return Ok(re
                    .replace(line, format!(r#"${{1}}{}${{2}}"#, self.new_name))
                    .to_string());
            }
        }

        // Single quotes: package = 'old-name'
        // Capture: (package = ')old-name(')
        let single_pattern = format!(r#"(\bpackage\s*=\s*'){}(')"#, regex::escape(self.old_name));
        if let Ok(re) = Regex::new(&single_pattern) {
            if re.is_match(line) {
                return Ok(re
                    .replace(line, format!(r#"${{1}}{}${{2}}"#, self.new_name))
                    .to_string());
            }
        }

        Ok(line.to_string())
    }

    fn update_standalone_path(&self, line: &str) -> Result<String> {
        if let Some(new_path) = self.new_path {
            // Match: path = "..." or path = '...'
            let pattern = r#"^(\s*path\s*=\s*)["'][^"']*["']"#;
            if let Ok(re) = Regex::new(pattern) {
                return Ok(re
                    .replace(line, format!(r#"${{1}}"{}""#, new_path))
                    .to_string());
            }
        }
        Ok(line.to_string())
    }

    fn update_inline_path(&self, line: &str) -> Result<String> {
        if let Some(new_path) = self.new_path {
            // Already has the new path?
            if line.contains(&format!(r#"path = "{}""#, new_path)) {
                return Ok(line.to_string());
            }

            // Match: path = "..." or path = '...' anywhere in the line
            let pattern = r#"(\bpath\s*=\s*)["'][^"']*["']"#;
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(line) {
                    return Ok(re
                        .replace(line, format!(r#"${{1}}"{}""#, new_path))
                        .to_string());
                }
            }
        }
        Ok(line.to_string())
    }
}

#[cfg(test)]
mod comprehensive_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_multiline_inline_table() {
        let input = r#"[dependencies]
my-crate = {
    path = "../old-path",
    features = ["feat1", "feat2"]
}
"#;
        let expected = r#"[dependencies]
my-crate = {
    path = "../new-path",
    features = ["feat1", "feat2"]
}
"#;

        let temp = TempDir::new().unwrap();
        let pkg_dir = temp.path().join("my-pkg");
        fs::create_dir(&pkg_dir).unwrap();
        let manifest = pkg_dir.join("Cargo.toml");
        fs::write(&manifest, input).unwrap();

        let new_dir = temp.path().join("new-path");

        let mut txn = Transaction::new(false);
        update_dependent_manifest(
            &manifest, "my-crate", "my-crate", &new_dir, true, false, &mut txn,
        )
        .unwrap();

        txn.commit().unwrap();
        let result = fs::read_to_string(&manifest).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_inline_comments() {
        let input = r#"[dependencies]
old-crate = { path = "../old-path" }  # Important dependency
other = "1.0"
"#;
        let expected = r#"[dependencies]
new-crate = { path = "../new-path" }  # Important dependency
other = "1.0"
"#;

        let temp = TempDir::new().unwrap();
        let pkg_dir = temp.path().join("my-pkg");
        fs::create_dir(&pkg_dir).unwrap();
        let manifest = pkg_dir.join("Cargo.toml");
        fs::write(&manifest, input).unwrap();

        let new_dir = temp.path().join("new-path");

        let mut txn = Transaction::new(false);
        update_dependent_manifest(
            &manifest,
            "old-crate",
            "new-crate",
            &new_dir,
            true,
            true,
            &mut txn,
        )
        .unwrap();

        txn.commit().unwrap();
        let result = fs::read_to_string(&manifest).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_target_specific_dependencies() {
        let input = r#"[target.'cfg(windows)'.dependencies]
old-crate = { path = "../old-path" }

[target.'cfg(unix)'.dev-dependencies]
other = "1.0"
"#;
        let expected = r#"[target.'cfg(windows)'.dependencies]
new-crate = { path = "../new-path" }

[target.'cfg(unix)'.dev-dependencies]
other = "1.0"
"#;

        let temp = TempDir::new().unwrap();
        let pkg_dir = temp.path().join("my-pkg");
        fs::create_dir(&pkg_dir).unwrap();
        let manifest = pkg_dir.join("Cargo.toml");
        fs::write(&manifest, input).unwrap();

        let new_dir = temp.path().join("new-path");

        let mut txn = Transaction::new(false);
        update_dependent_manifest(
            &manifest,
            "old-crate",
            "new-crate",
            &new_dir,
            true,
            true,
            &mut txn,
        )
        .unwrap();

        txn.commit().unwrap();
        let result = fs::read_to_string(&manifest).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_single_quotes() {
        let input = r#"[dependencies]
old-crate = { path = '../old-path', version = "1.0" }
"#;
        let expected = r#"[dependencies]
new-crate = { path = "../new-path", version = "1.0" }
"#;

        let temp = TempDir::new().unwrap();
        let pkg_dir = temp.path().join("my-pkg");
        fs::create_dir(&pkg_dir).unwrap();
        let manifest = pkg_dir.join("Cargo.toml");
        fs::write(&manifest, input).unwrap();

        let new_dir = temp.path().join("new-path");

        let mut txn = Transaction::new(false);
        update_dependent_manifest(
            &manifest,
            "old-crate",
            "new-crate",
            &new_dir,
            true,
            true,
            &mut txn,
        )
        .unwrap();

        txn.commit().unwrap();
        let result = fs::read_to_string(&manifest).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_optional_dependency() {
        let input = r#"[dependencies]
old-crate = { path = "../old-path", optional = true }
"#;
        let expected = r#"[dependencies]
new-crate = { path = "../new-path", optional = true }
"#;

        let temp = TempDir::new().unwrap();
        let pkg_dir = temp.path().join("my-pkg");
        fs::create_dir(&pkg_dir).unwrap();
        let manifest = pkg_dir.join("Cargo.toml");
        fs::write(&manifest, input).unwrap();

        let new_dir = temp.path().join("new-path");

        let mut txn = Transaction::new(false);
        update_dependent_manifest(
            &manifest,
            "old-crate",
            "new-crate",
            &new_dir,
            true,
            true,
            &mut txn,
        )
        .unwrap();

        txn.commit().unwrap();
        let result = fs::read_to_string(&manifest).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_package_aliases() {
        let input = r#"[dependencies]
alias1 = { package = "old-crate", path = "../old-path" }
alias2 = { package = "old-crate", version = "1.0" }
"#;
        let expected = r#"[dependencies]
alias1 = { package = "new-crate", path = "../new-path" }
alias2 = { package = "new-crate", version = "1.0" }
"#;

        let temp = TempDir::new().unwrap();
        let pkg_dir = temp.path().join("my-pkg");
        fs::create_dir(&pkg_dir).unwrap();
        let manifest = pkg_dir.join("Cargo.toml");
        fs::write(&manifest, input).unwrap();

        let new_dir = temp.path().join("new-path");

        let mut txn = Transaction::new(false);
        update_dependent_manifest(
            &manifest,
            "old-crate",
            "new-crate",
            &new_dir,
            true,
            true,
            &mut txn,
        )
        .unwrap();

        txn.commit().unwrap();
        let result = fs::read_to_string(&manifest).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_workspace_dep_with_features() {
        let input = r#"[dependencies]
old-crate = { workspace = true, features = ["extra"] }
"#;
        let expected = r#"[dependencies]
new-crate = { workspace = true, features = ["extra"] }
"#;

        let temp = TempDir::new().unwrap();
        let manifest = temp.path().join("Cargo.toml");
        fs::write(&manifest, input).unwrap();

        let mut txn = Transaction::new(false);
        update_dependent_manifest(
            &manifest,
            "old-crate",
            "new-crate",
            temp.path(), // path doesn't matter for workspace deps
            false,       // don't change path
            true,        // change name
            &mut txn,
        )
        .unwrap();

        txn.commit().unwrap();
        let result = fs::read_to_string(&manifest).unwrap();
        assert_eq!(result, expected);
    }
}
