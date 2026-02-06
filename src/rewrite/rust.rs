//! Rust source code rewriting for crate renames.
//!
//! Updates `.rs` and `.md` files when a crate is renamed, using regex patterns
//! to match Rust syntax contexts (use statements, paths, attributes, doc links).
//!
//! ## Approach
//!
//! Uses regex instead of AST manipulation to:
//! - Preserve formatting, comments, and whitespace exactly
//! - Handle edge cases parsers might reject
//! - Avoid parsing overhead
//!
//! Patterns use word boundaries (`\b`) to prevent false positives.
//!
//! ## Supported Contexts
//!
//! ```rust,ignore
//! use old_crate;                    // Use statements
//! use old_crate::module;            // Module paths
//! use old_crate::{a, b};            // Use groups
//! old_crate::function()             // Qualified calls
//! #[old_crate::attribute]           // Attributes
//! /// See [`old_crate::Type`]       // Doc links
//! extern crate old_crate;           // 2015 edition
//! ```

use crate::error::Result;
use crate::fs::transaction::Transaction;
use cargo_metadata::Metadata;
use regex::Regex;
use std::fs;
use std::path::Path;

/// Updates source code references in workspace packages.
///
/// Scans all `.rs` and `.md` files, applying regex replacements for the renamed crate.
pub fn update_source_code(
    metadata: &Metadata,
    old_name: &str,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let old_snake = old_name.replace('-', "_");
    let new_snake = new_name.replace('-', "_");

    let patterns = RenamePatterns::new(&old_snake, &new_snake)?;

    for member in metadata.workspace_packages() {
        let pkg_root = member
            .manifest_path
            .parent()
            .expect("manifest path must have parent");

        walk_package(pkg_root.as_std_path(), &patterns, txn)?;
    }

    Ok(())
}

/// Compiled regex patterns for crate references.
struct RenamePatterns {
    old_snake: String,
    new_snake: String,
    replacements: Vec<(Regex, String)>,
}

impl RenamePatterns {
    /// Compiles all patterns for the rename operation.
    fn new(old_snake: &str, new_snake: &str) -> Result<Self> {
        let old_escaped = regex::escape(old_snake);
        let mut replacements = Vec::new();

        // 1. Use statements: use old_crate
        replacements.push((
            Regex::new(&format!(r"\b(use\s+){old}(::|;|\s+as)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 2. Absolute paths (2015/2018): ::old_crate
        replacements.push((
            Regex::new(&format!(r"\b(::{old})(::|;|\s+as)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 3. Extern crate (2015): extern crate old_crate
        replacements.push((
            Regex::new(&format!(
                r"\b(extern\s+crate\s+){old}(::|;|\s+as)",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 4. Qualified paths: old_crate::path
        replacements.push((
            Regex::new(&format!(r"\b{old}(::)", old = old_escaped))?,
            format!("{new}${{1}}", new = new_snake),
        ));

        // 5. Absolute paths: ::old_crate::
        replacements.push((
            Regex::new(&format!(r"(::){old}(::)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 6. Attributes: #[old_crate::attr] or #[derive(old_crate::Derive)]
        replacements.push((
            Regex::new(&format!(r"(#\[(?:derive\()?){old}(::)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 7. Attributes with parens: #[old_crate(...)]
        replacements.push((
            Regex::new(&format!(r"(#\[){old}(\()", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 8. Doc links: [`old_crate::Type`] or [`old_crate`]
        replacements.push((
            Regex::new(&format!(r"(`){old}([::`\]])", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 9. Use with self: use old_crate::{self, ...}
        replacements.push((
            Regex::new(&format!(r"\b(use\s+){old}(::self\b)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}${{3}}", new = new_snake),
        ));

        // 10. Raw identifiers: r#old_crate
        replacements.push((
            Regex::new(&format!(r"\br#{old}\b", old = old_escaped))?,
            format!("r#{new}", new = new_snake),
        ));

        // 11. Crate-specific macros: old_crate_something!
        replacements.push((
            Regex::new(&format!(r"\b{old}([a-z_][a-z0-9_]*)!", old = old_escaped))?,
            format!("{new}${{1}}", new = new_snake),
        ));

        Ok(Self {
            old_snake: old_snake.to_string(),
            new_snake: new_snake.to_string(),
            replacements,
        })
    }

    /// Applies all patterns to content.
    ///
    /// Returns `Some(modified)` if any pattern matched, `None` otherwise.
    fn apply(&self, content: &str) -> Option<String> {
        let mut result = content.to_string();
        let mut changed = false;

        for (pattern, replacement) in &self.replacements {
            if pattern.is_match(&result) {
                result = pattern.replace_all(&result, replacement).to_string();
                changed = true;
            }
        }

        if changed { Some(result) } else { None }
    }
}

/// Recursively walks a package directory, processing source files.
fn walk_package(root: &Path, patterns: &RenamePatterns, txn: &mut Transaction) -> Result<()> {
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .filter_entry(|e| {
            let name = e.file_name().to_str();
            // Skip target and .git directories
            !(name == Some("target") || name == Some(".git"))
        })
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::debug!("Skipping entry due to error: {}", e);
                continue;
            }
        };

        // Process only regular files
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let path = entry.path();
        match path.extension().and_then(|s| s.to_str()) {
            Some("rs") => update_rust_file(path, patterns, txn)?,
            Some("md") => update_doc_file(path, patterns, txn)?,
            _ => {}
        }
    }

    Ok(())
}

/// Updates a single Rust source file.
fn update_rust_file(path: &Path, patterns: &RenamePatterns, txn: &mut Transaction) -> Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            log::debug!("Skipping file (read error): {} - {}", path.display(), e);
            return Ok(());
        }
    };

    if syn::parse_file(&content).is_err() {
        log::debug!("Skipping file (invalid syntax): {}", path.display());
        return Ok(());
    }

    if let Some(new_content) = patterns.apply(&content) {
        txn.update_file(path.to_path_buf(), new_content)?;
        log::debug!("Updated Rust file: {}", path.display());
    }

    Ok(())
}

/// Updates a documentation file (.md or .txt).
///
/// Replaces kebab-case crate names (for Markdown/docs).
fn update_doc_file(path: &Path, patterns: &RenamePatterns, txn: &mut Transaction) -> Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            log::debug!("Skipping file (read error): {} - {}", path.display(), e);
            return Ok(());
        }
    };

    // Convert snake_case to kebab-case for Markdown
    let old_kebab = patterns.old_snake.replace('_', "-");
    let new_kebab = patterns.new_snake.replace('_', "-");

    // Match whole words only
    let doc_pattern = Regex::new(&format!(r"\b{}\b", regex::escape(&old_kebab)))?;

    if doc_pattern.is_match(&content) {
        let new_content = doc_pattern.replace_all(&content, &new_kebab).into_owned();

        if new_content != content {
            txn.update_file(path.to_path_buf(), new_content)?;
            log::debug!("Updated doc file: {}", path.display());
        }
    }

    Ok(())
}
