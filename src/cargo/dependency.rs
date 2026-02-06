//! Dependency reference updates in `Cargo.toml` files.
//!
//! This module handles the complex task of updating dependency declarations
//! when a package is renamed or moved. It supports all Cargo dependency formats
//! including inline tables, multi-line tables, target-specific dependencies,
//! and workspace inheritance.
//!
//! # Supported Formats
//!
//! ## Inline Tables
//! ```toml
//! [dependencies]
//! my-crate = { path = "../my-crate", version = "0.1" }
//! ```
//!
//! ## Multi-Line Inline Tables
//! ```toml
//! my-crate = {
//!     path = "../my-crate",
//!     features = ["feat1", "feat2"]
//! }
//! ```
//!
//! ## Multi-Line Tables
//! ```toml
//! [dependencies.my-crate]
//! path = "../my-crate"
//! features = ["feat1"]
//! ```
//!
//! ## Target-Specific
//! ```toml
//! [target.'cfg(windows)'.dependencies]
//! my-crate = { path = "../my-crate" }
//!
//! [target.x86_64-unknown-linux-gnu.dependencies]
//! my-crate = { path = "../my-crate" }
//! ```
//!
//! ## Package Renames
//! ```toml
//! alias = { package = "my-crate", path = "../my-crate" }
//! ```
//!
//! ## Workspace Inheritance
//! ```toml
//! my-crate = { workspace = true }
//! ```
//!
//! # State Machine
//!
//! `TomlProcessor` is a line-by-line state machine that tracks:
//!
//! - **Current section**: Which `[dependencies]` section we're in
//! - **Brace depth**: Whether we're inside a multi-line inline table `{ ... }`
//! - **Target context**: Whether we're processing the renamed dependency
//!
//! State transitions occur when:
//! - Section headers are encountered (`[dependencies]`)
//! - Dependency declarations are found (`my-crate = ...`)
//! - Braces open/close in inline tables
//!
//! # Guarantees
//!
//! - **Preserves formatting**: Whitespace, indentation, and alignment unchanged
//! - **Preserves comments**: Both inline (`# comment`) and block comments
//! - **Preserves trailing newlines**: Files with/without final `\n` remain unchanged
//! - **Atomic updates**: All changes via transaction, rollback on error
//! - **Path normalization**: Converts backslashes to forward slashes

use crate::error::Result;
use crate::fs::transaction::Transaction;
use regex::Regex;
use std::fs;
use std::path::Path;

/// Updates dependency references in a package's `Cargo.toml`.
///
/// Scans the manifest for references to `old_name` and updates them to `new_name`
/// and/or `new_dir`. Handles all dependency formats (see module docs).
///
/// # Arguments
///
/// - `manifest_path`: Path to the dependent package's `Cargo.toml`
/// - `old_name`: Current name of the dependency
/// - `new_name`: New name of the dependency
/// - `new_dir`: New directory of the dependency (absolute path)
/// - `path_changed`: Whether the dependency's directory changed
/// - `name_changed`: Whether the dependency's name changed
///
/// # Errors
///
/// - `Io`: Cannot read manifest file
/// - `Other`: Regex compilation failure (indicates bug in patterns)
///
/// # Examples
///
/// ```no_run
/// # use cargo_rename::cargo::dependency::update_dependent_manifest;
/// # use cargo_rename::fs::Transaction;
/// # use std::path::Path;
/// # fn example() -> cargo_rename::error::Result<()> {
/// let mut txn = Transaction::new(false);
/// update_dependent_manifest(
///     Path::new("app/Cargo.toml"),
///     "old-lib",
///     "new-lib",
///     Path::new("/workspace/new-lib"),
///     true,  // path changed
///     true,  // name changed
///     &mut txn
/// )?;
/// txn.commit()?;
/// # Ok(())
/// # }
/// ```
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
                    modified_line = self.rename_section_header(line)?;
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
                modified_line = self.update_standalone_path(line)?;
                result_lines.push(modified_line);
                continue;
            }

            // Handle dependency declaration lines - always search for old name
            if self.is_dependency_line(trimmed, search_dep) {
                self.start_dependency_tracking(line, search_dep);

                if name_changed {
                    modified_line = self.rename_dependency_key(line)?;
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
                    modified_line = self.update_inline_path(line)?;
                }
                self.update_brace_depth(line);
                result_lines.push(modified_line);
                continue;
            }

            // Handle lines with package field
            if name_changed && self.has_package_field(line) {
                self.start_dependency_tracking(line, search_dep);
                modified_line = self.rename_package_field(line)?;

                if path_changed && self.has_path_field(line) {
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
        if header.starts_with("[target.")
            && let Some(target) = self.extract_target_triple(header)
        {
            return Some(DependencySection::TargetDependencies(target));
        }

        None
    }

    fn extract_target_triple(&self, header: &str) -> Option<String> {
        // Try quoted first: [target.'cfg(windows)'.dependencies]
        let quoted_pattern = Regex::new(r"\[target\.'([^']+)'\.").ok()?;
        if let Some(caps) = quoted_pattern.captures(header) {
            return caps.get(1).map(|m| m.as_str().to_string());
        }

        // Try unquoted: [target.x86_64-unknown-linux-gnu.dependencies]
        let unquoted_pattern = Regex::new(
            r"\[target\.([^.\]]+)\.(?:dependencies|dev-dependencies|build-dependencies)\]",
        )
        .ok()?;
        unquoted_pattern
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
        if let Ok(re) = Regex::new(&key_pattern)
            && re.is_match(line)
        {
            self.in_target_dep = true;
            self.in_package_dep = false;
            self.update_brace_depth(line);
            return;
        }

        // Check if this has package = "target_dep"
        let package_pattern = format!(r#"package\s*=\s*["']{}["']"#, regex::escape(target_dep));
        if let Ok(re) = Regex::new(&package_pattern)
            && re.is_match(line)
        {
            self.in_package_dep = true;
            self.in_target_dep = false;
            self.update_brace_depth(line);
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

            if let Ok(re) = Regex::new(&pattern)
                && re.is_match(line)
            {
                return Ok(re
                    .replace(line, format!("${{1}}{}]", self.new_name))
                    .to_string());
            }
        }

        Ok(line.to_string())
    }

    fn rename_dependency_key(&self, line: &str) -> Result<String> {
        // old-name.workspace = true
        let ws_pattern = format!(
            r"^(\s*){}\s*\.\s*workspace\s*=",
            regex::escape(self.old_name)
        );
        if let Ok(re) = Regex::new(&ws_pattern)
            && re.is_match(line)
        {
            return Ok(re
                .replace(line, format!("${{1}}{}.workspace =", self.new_name))
                .to_string());
        }

        // old-name = ...
        let key_pattern = format!(r"^(\s*){}\s*=\s*", regex::escape(self.old_name));
        if let Ok(re) = Regex::new(&key_pattern)
            && re.is_match(line)
        {
            return Ok(re
                .replace(line, format!("${{1}}{} = ", self.new_name))
                .to_string());
        }

        Ok(line.to_string())
    }

    fn rename_package_field(&self, line: &str) -> Result<String> {
        // Double quotes: package = "old-name"
        // Captures (package = ")old-name(")
        let double_pattern = format!(r#"(\bpackage\s*=\s*"){}(")"#, regex::escape(self.old_name));
        if let Ok(re) = Regex::new(&double_pattern)
            && re.is_match(line)
        {
            return Ok(re
                .replace(line, format!(r#"${{1}}{}${{2}}"#, self.new_name))
                .to_string());
        }

        // Single quotes: package = 'old-name'
        // Captures (package = ')old-name(')
        let single_pattern = format!(r#"(\bpackage\s*=\s*'){}(')"#, regex::escape(self.old_name));
        if let Ok(re) = Regex::new(&single_pattern)
            && re.is_match(line)
        {
            return Ok(re
                .replace(line, format!(r#"${{1}}{}${{2}}"#, self.new_name))
                .to_string());
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

            // Match path = "..." or path = '...' anywhere in the line
            let pattern = r#"(\bpath\s*=\s*)["'][^"']*["']"#;
            if let Ok(re) = Regex::new(pattern)
                && re.is_match(line)
            {
                return Ok(re
                    .replace(line, format!(r#"${{1}}"{}""#, new_path))
                    .to_string());
            }
        }
        Ok(line.to_string())
    }
}

#[cfg(test)]
mod tests {
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
