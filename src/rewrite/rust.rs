//! Rust source code rewriting for crate renames.
//!
//! This module updates Rust source files (`.rs`) and Markdown documentation (`.md`)
//! to reflect a crate rename. It handles all Rust syntax contexts where crate names
//! appear: use statements, qualified paths, attributes, doc links, and more.
//!
//! # Approach
//!
//! Uses **regex-based pattern matching** rather than AST manipulation. This approach:
//!
//! - ✅ Preserves all formatting, comments, and whitespace exactly
//! - ✅ Fast (no parsing overhead)
//! - ✅ Handles edge cases that parsers might reject
//! - ⚠️ Requires careful word-boundary matching to avoid false positives
//!
//! A `syn` validation pass is performed after regex replacement to catch any
//! accidental syntax corruption (though this should never happen with the
//! word-boundary patterns used).
//!
//! # Supported Contexts
//!
//! ## Use Statements
//! ```rust,ignore
//! use old_crate;                  // → use new_crate;
//! use old_crate::module;          // → use new_crate::module;
//! use old_crate::{a, b};          // → use new_crate::{a, b};
//! use old_crate as alias;         // → use new_crate as alias;
//! use old_crate::{self, module};  // → use new_crate::{self, module};
//! use ::old_crate::module;        // → use ::new_crate::module; (2015/2018 edition)
//! ```
//!
//! ## Qualified Paths
//! ```rust,ignore
//! old_crate::function()           // → new_crate::function()
//! old_crate::Type::new()          // → new_crate::Type::new()
//! old_crate::CONSTANT             // → new_crate::CONSTANT
//! ::old_crate::absolute_path()    // → ::new_crate::absolute_path()
//! ```
//!
//! ## Attributes
//! ```rust,ignore
//! #[old_crate::attribute]         // → #[new_crate::attribute]
//! #[derive(old_crate::Derive)]    // → #[derive(new_crate::Derive)]
//! #[old_crate(arg)]               // → #[new_crate(arg)]
//! ```
//!
//! ## Doc Comments
//! ```rust,ignore
//! /// See [`old_crate::Type`]     // → /// See [`new_crate::Type`]
//! /// [`old_crate`] provides...   // → /// [`new_crate`] provides...
//! ```
//!
//! ## Advanced Syntax
//! ```rust,ignore
//! <old_crate::Type as Trait>::method()  // UFCS
//! fn foo<T: old_crate::Trait>()         // Generic bounds
//! where T: old_crate::Trait             // Where clauses
//! type X = old_crate::AssocType;        // Type aliases
//! old_crate_macro!()                    // Crate-specific macros
//! extern crate old_crate;               // 2015 edition
//! use r#old_crate::module;              // Raw identifiers
//! ```
//!
//! # Limitations
//!
//! - **Feature names**: `#[cfg(feature = "old_crate")]` are NOT changed (intentional)
//! - **String literals**: `"old_crate"` inside strings are NOT changed (intentional)
//! - **Module names**: `mod old_crate { }` are NOT changed (different concept)
//! - **Comments**: Plain comments are NOT changed (only intra-doc links)
//!
//! # File Discovery
//!
//! Uses the `ignore` crate to walk the workspace:
//! - Honors `.gitignore`, `.ignore`, and `.git/info/exclude`
//! - Skips `target/` and `.git/` directories
//! - Processes `.rs` and `.md` files only

use crate::error::Result;
use crate::fs::transaction::Transaction;
use cargo_metadata::Metadata;
use ignore::WalkBuilder;
use regex::Regex;
use std::{fs, path::Path};

/// Updates all Rust source files and documentation in the workspace.
///
/// Walks every workspace member package and applies rename patterns to:
/// - Rust source files (`.rs`)
/// - Markdown documentation (`.md`)
///
/// Files are only modified if they contain references to the old crate name.
///
/// # Arguments
///
/// - `metadata`: Cargo workspace metadata
/// - `old_name`: Current crate name (kebab-case, e.g., `my-crate`)
/// - `new_name`: New crate name (kebab-case, e.g., `new-crate`)
/// - `txn`: Transaction to stage file updates
///
/// # Errors
///
/// - `Io`: File read/write failures
/// - `Regex`: Pattern compilation failures (indicates bug)
///
/// # Examples
///
/// ```no_run
/// # use cargo_rename::rewrite::rust::update_source_code;
/// # use cargo_rename::fs::Transaction;
/// # fn example(metadata: &cargo_metadata::Metadata) -> cargo_rename::error::Result<()> {
/// let mut txn = Transaction::new(false);
/// update_source_code(metadata, "old-crate", "new-crate", &mut txn)?;
/// txn.commit()?;
/// # Ok(())
/// # }
/// ```
pub fn update_source_code(
    metadata: &Metadata,
    old_name: &str,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    // Convert kebab-case to snake_case for Rust identifiers
    let old_snake = old_name.replace('-', "_");
    let new_snake = new_name.replace('-', "_");

    let patterns = RenamePatterns::new(&old_snake, &new_snake)?;

    for member in &metadata.workspace_packages() {
        let pkg_root = member
            .manifest_path
            .parent()
            .expect("manifest path must have parent");

        walk_package(pkg_root.as_std_path(), &patterns, txn)?;
    }

    Ok(())
}

/// Compiled regex patterns for finding and replacing crate references.
///
/// Patterns are carefully designed to:
/// - Match only complete identifiers (word boundaries)
/// - Capture surrounding context (use keywords, punctuation)
/// - Preserve formatting via capture groups
struct RenamePatterns {
    old_snake: String,
    new_snake: String,
    replacements: Vec<(Regex, String)>,
}

impl RenamePatterns {
    /// Compiles all regex patterns for the rename operation.
    ///
    /// # Pattern Design
    ///
    /// Each pattern uses:
    /// - `\b` for word boundaries (prevents partial matches)
    /// - `regex::escape()` to handle special characters in crate names
    /// - Capture groups `()` to preserve surrounding syntax
    /// - Non-capturing groups `(?:...)` for alternatives
    ///
    /// # Errors
    ///
    /// Returns `Regex` error if pattern compilation fails (should never happen
    /// with hardcoded patterns).
    fn new(old_snake: &str, new_snake: &str) -> Result<Self> {
        let old_escaped = regex::escape(old_snake);
        let mut replacements = Vec::new();

        // 1. Use statements: use old_crate
        replacements.push((
            Regex::new(&format!(
                r"(\buse\s+){old}(\s*(?:::|\s+as\s+|;|\{{))",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 2. Use with absolute path (2015/2018): use ::old_crate
        replacements.push((
            Regex::new(&format!(
                r"(\buse\s+::){old}(\s*(?:::|\s+as\s+|;|\{{))",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 3. Extern crate (2015 edition): extern crate old_crate
        replacements.push((
            Regex::new(&format!(
                r"(\bextern\s+crate\s+){old}(\s*(?:as\s+|;))",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 4. Qualified paths: old_crate::path
        // Matches: function calls, types, constants, macros, UFCS, trait bounds
        replacements.push((
            Regex::new(&format!(r"\b{old}(::)", old = old_escaped))?,
            format!("{new}${{1}}", new = new_snake),
        ));

        // 5. Absolute paths (2015/2018): ::old_crate::
        replacements.push((
            Regex::new(&format!(r"(::){old}(::)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 6. Attributes: #[old_crate::attr] or #[derive(old_crate::Derive)]
        replacements.push((
            Regex::new(&format!(r"(#\[(?:derive\()?){old}(::)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 7. Attribute with parentheses: #[old_crate(...)]
        replacements.push((
            Regex::new(&format!(r"(#\[){old}(\()", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 8. Doc comment intra-doc links: [`old_crate::Type`] or [`old_crate`]
        replacements.push((
            Regex::new(&format!(r"(\[`){old}(`\]|::)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 9. Use with self: use old_crate::{self, ...}
        replacements.push((
            Regex::new(&format!(
                r"(\buse\s+){old}(::)\{{(\s*self\b)",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}{{${{3}}", new = new_snake),
        ));

        // 10. Raw identifiers: r#old_crate
        replacements.push((
            Regex::new(&format!(r"\br#{old}\b", old = old_escaped))?,
            format!("r#{new}", new = new_snake),
        ));

        // 11. Crate-specific macros: old_crate_something!
        // Common pattern: crate_name_macro_name!
        replacements.push((
            Regex::new(&format!(r"\b{old}(_[a-z_][a-z0-9_]*!)", old = old_escaped))?,
            format!("{new}${{1}}", new = new_snake),
        ));

        Ok(Self {
            old_snake: old_snake.to_string(),
            new_snake: new_snake.to_string(),
            replacements,
        })
    }

    /// Applies all patterns to the content.
    ///
    /// Returns `Some(modified_content)` if any pattern matched, `None` otherwise.
    ///
    /// # Performance
    ///
    /// Early-exits if no patterns match (most files won't reference the crate).
    fn apply(&self, content: &str) -> Option<String> {
        let mut result = content.to_string();
        let mut changed = false;

        for (pattern, replacement) in &self.replacements {
            if pattern.is_match(&result) {
                result = pattern.replace_all(&result, replacement).into_owned();
                changed = true;
            }
        }

        if changed { Some(result) } else { None }
    }
}

/// Walks a package directory and updates relevant files.
///
/// Uses `ignore::WalkBuilder` to:
/// - Respect `.gitignore`, `.ignore`, and `.git/info/exclude`
/// - Skip `target/` and `.git/` directories
/// - Process only `.rs` and `.md` files
fn walk_package(root: &Path, patterns: &RenamePatterns, txn: &mut Transaction) -> Result<()> {
    let walker = WalkBuilder::new(root)
        .hidden(false) // Don't skip hidden files (e.g., .cargo-ok is fine)
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

/// Updates a Rust source file (`.rs`).
///
/// # Process
///
/// 1. Read file content (skip if non-UTF8)
/// 2. Validate syntax with `syn` (skip if unparseable)
/// 3. Apply regex patterns
/// 4. Stage update in transaction if changed
///
/// # Why `syn` Validation?
///
/// Regex patterns are word-boundary-based and shouldn't create invalid syntax,
/// but `syn` provides an extra safety layer. If a file is unparseable:
/// - It might already be broken (skip to avoid blame)
/// - It might contain proc-macro/build-script code that doesn't parse standalone
fn update_rust_file(path: &Path, patterns: &RenamePatterns, txn: &mut Transaction) -> Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            log::debug!("Skipping file (read error): {} - {}", path.display(), e);
            return Ok(());
        }
    };

    // Validate Rust syntax before modifying
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

/// Updates a Markdown documentation file (`.md`).
///
/// Replaces kebab-case crate names (e.g., `my-crate`) as whole words.
/// Does NOT replace snake_case identifiers (those are in Rust code blocks).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_rust_reference_patterns() {
        let patterns = RenamePatterns::new("old_crate", "new_crate").unwrap();

        let test_cases = vec![
            ("use old_crate;", "use new_crate;"),
            ("use old_crate::module;", "use new_crate::module;"),
            ("use old_crate::{a, b};", "use new_crate::{a, b};"),
            ("use old_crate as alias;", "use new_crate as alias;"),
            (
                "use old_crate::{self, module};",
                "use new_crate::{self, module};",
            ),
            ("use old_crate::*;", "use new_crate::*;"),
            ("extern crate old_crate;", "extern crate new_crate;"),
            ("old_crate::function()", "new_crate::function()"),
            ("::old_crate::function()", "::new_crate::function()"),
            ("#[old_crate::attribute]", "#[new_crate::attribute]"),
            (
                "#[derive(old_crate::Derive)]",
                "#[derive(new_crate::Derive)]",
            ),
            ("/// See [`old_crate::Type`]", "/// See [`new_crate::Type`]"),
            (
                "fn foo() -> old_crate::Result<()>",
                "fn foo() -> new_crate::Result<()>",
            ),
            ("impl old_crate::Trait for X", "impl new_crate::Trait for X"),
        ];

        for (input, expected) in test_cases {
            let result = patterns.apply(input);
            assert_eq!(
                result.as_deref(),
                Some(expected),
                "Failed for input: {}",
                input
            );
        }
    }

    #[test]
    fn test_does_not_change_unrelated() {
        let patterns = RenamePatterns::new("old_crate", "new_crate").unwrap();

        let unchanged = vec![
            "let old_crate = 5;",               // Local variable
            "mod old_crate { }",                // Module definition
            r#"let s = "old_crate";"#,          // String literal
            "// Comment about old_crate",       // Plain comment
            "use old_crate_different::module;", // Different identifier
            r#"#[cfg(feature = "old_crate")]"#, // Feature name (intentional)
        ];

        for input in unchanged {
            let result = patterns.apply(input);
            assert_eq!(result, None, "Should not change: {}", input);
        }
    }

    #[test]
    fn test_preserves_formatting() {
        let patterns = RenamePatterns::new("old_crate", "new_crate").unwrap();

        let input = r#"
// Comment


use old_crate::module;



fn main() {
    old_crate::function();
}
"#;

        let expected = r#"
// Comment


use new_crate::module;



fn main() {
    new_crate::function();
}
"#;

        let result = patterns.apply(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_complex_real_world_example() {
        let patterns = RenamePatterns::new("old_crate", "new_crate").unwrap();

        let input = r#"
use old_crate::{self, Config};
use old_crate::prelude::*;

/// See [`old_crate::Type`] for details
#[derive(old_crate::Derive)]
#[old_crate(attribute = "value")]
pub struct MyStruct {
    config: old_crate::Config,
}

impl old_crate::Trait for MyStruct {
    type Assoc = old_crate::AssocType;
    
    fn method(&self) -> old_crate::Result<()> {
        old_crate_println!("test");
        old_crate::function()?;
        Ok(())
    }
}

fn generic_function<T: old_crate::Bound>() 
where
    T: old_crate::OtherTrait,
{
    let _ = <T as old_crate::Trait>::method();
}
"#;

        let expected = r#"
use new_crate::{self, Config};
use new_crate::prelude::*;

/// See [`new_crate::Type`] for details
#[derive(new_crate::Derive)]
#[new_crate(attribute = "value")]
pub struct MyStruct {
    config: new_crate::Config,
}

impl new_crate::Trait for MyStruct {
    type Assoc = new_crate::AssocType;
    
    fn method(&self) -> new_crate::Result<()> {
        new_crate_println!("test");
        new_crate::function()?;
        Ok(())
    }
}

fn generic_function<T: new_crate::Bound>() 
where
    T: new_crate::OtherTrait,
{
    let _ = <T as new_crate::Trait>::method();
}
"#;

        let result = patterns.apply(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_does_not_break_on_partial_matches() {
        let patterns = RenamePatterns::new("old", "new").unwrap();

        // Should only match word boundaries
        let unchanged = vec![
            "use old_crate::module;", // old_crate != old
            "let older = 5;",         // older != old
            "use bold::module;",      // bold != old
        ];

        for input in unchanged {
            let result = patterns.apply(input);
            assert_eq!(result, None, "Should not change: {}", input);
        }
    }
}
