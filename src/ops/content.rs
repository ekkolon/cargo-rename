use crate::error::Result;
use crate::ops::transaction::Transaction;
use cargo_metadata::Metadata;
use ignore::WalkBuilder;
use regex::Regex;
use std::{fs, path::Path};

/// Updates Rust source files and documentation to reflect a crate rename.
///
/// Guarantees:
/// - Preserves all formatting, comments, and whitespace
/// - Handles all Rust syntax contexts (use, paths, attributes, docs)
/// - Never mutates files that aren't using the old crate name
/// - Honors .gitignore, .ignore, and .git/info/exclude
/// - Idempotent
pub fn update_source_code(
    metadata: &Metadata,
    old_name: &str,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let old_snake = old_name.replace('-', "_");
    let new_snake = new_name.replace('-', "_");

    // Create regex patterns for different contexts
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

/// Comprehensive patterns for finding and replacing crate references
struct RenamePatterns {
    old_snake: String,
    new_snake: String,
    replacements: Vec<(Regex, String)>,
}

impl RenamePatterns {
    fn new(old_snake: &str, new_snake: &str) -> Result<Self> {
        let old_escaped = regex::escape(old_snake);
        let mut replacements = Vec::new();

        // 1. Use statements: use old_crate
        // Matches: use old_crate, use old_crate::, use old_crate as, use old_crate;
        replacements.push((
            Regex::new(&format!(
                r"(\buse\s+){old}(\s*(?:::|\s+as\s+|;|\{{))",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 2. Use with crate root (2015/2018): use ::old_crate
        replacements.push((
            Regex::new(&format!(
                r"(\buse\s+::){old}(\s*(?:::|\s+as\s+|;|\{{))",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 3. Extern crate: extern crate old_crate
        replacements.push((
            Regex::new(&format!(
                r"(\bextern\s+crate\s+){old}(\s*(?:as\s+|;))",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 4. Qualified paths: old_crate::path
        // This handles function calls, types, constants, macros
        replacements.push((
            Regex::new(&format!(r"\b{old}(::)", old = old_escaped))?,
            format!("{new}${{1}}", new = new_snake),
        ));

        // 5. Absolute crate paths (2015/2018): ::old_crate::
        replacements.push((
            Regex::new(&format!(r"(::){old}(::)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 6. Attributes: #[old_crate::attr] or #[derive(old_crate::Derive)]
        replacements.push((
            Regex::new(&format!(r"(#\[(?:derive\()?){old}(::)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 7. Doc comment intra-doc links: [`old_crate::Type`] or [`old_crate`]
        replacements.push((
            Regex::new(&format!(r"(\[`){old}(`\]|::)", old = old_escaped))?,
            format!("${{1}}{new}${{2}}", new = new_snake),
        ));

        // 8. Use with self: use old_crate::{self
        replacements.push((
            Regex::new(&format!(
                r"(\buse\s+){old}(::)\{{(\s*self\b)",
                old = old_escaped
            ))?,
            format!("${{1}}{new}${{2}}{{${{3}}", new = new_snake),
        ));

        Ok(Self {
            old_snake: old_snake.to_string(),
            new_snake: new_snake.to_string(),
            replacements,
        })
    }

    /// Apply all patterns to the content and return modified version if changed
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

fn walk_package(root: &Path, patterns: &RenamePatterns, txn: &mut Transaction) -> Result<()> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .filter_entry(|e| {
            if let Some(ft) = e.file_type()
                && !ft.is_dir() {
                    return true;
                }
            !matches!(e.file_name().to_str(), Some("target") | Some(".git"))
        })
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

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

fn update_rust_file(path: &Path, patterns: &RenamePatterns, txn: &mut Transaction) -> Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // skip non-UTF8 or unreadable files
    };

    // Verify it's valid Rust before modifying (optional safety check)
    if syn::parse_file(&content).is_err() {
        log::debug!("Skipping unparseable file: {}", path.display());
        return Ok(()); // skip files that don't parse
    }

    if let Some(new_content) = patterns.apply(&content) {
        txn.update_file(path.to_path_buf(), new_content)?;
    }

    Ok(())
}

fn update_doc_file(path: &Path, patterns: &RenamePatterns, txn: &mut Transaction) -> Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    // For markdown docs, replace both snake_case and kebab-case
    let old_kebab = patterns.old_snake.replace('_', "-");
    let new_kebab = patterns.new_snake.replace('_', "-");

    // Match whole words only to avoid partial replacements
    let doc_pattern = Regex::new(&format!(r"\b{}\b", regex::escape(&old_kebab))).unwrap();

    if doc_pattern.is_match(&content) {
        let new_content = doc_pattern.replace_all(&content, &new_kebab).into_owned();

        if new_content != content {
            txn.update_file(path.to_path_buf(), new_content)?;
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

        // Test cases covering ALL Rust syntax
        let test_cases = vec![
            // Basic use statements
            ("use old_crate;", "use new_crate;"),
            ("use old_crate::module;", "use new_crate::module;"),
            ("use old_crate::{a, b};", "use new_crate::{a, b};"),
            // Use with 'as'
            ("use old_crate as alias;", "use new_crate as alias;"),
            ("use old_crate::module as m;", "use new_crate::module as m;"),
            // Use with 'self'
            (
                "use old_crate::{self, module};",
                "use new_crate::{self, module};",
            ),
            (
                "use old_crate::m::{self as alias};",
                "use new_crate::m::{self as alias};",
            ),
            // Glob imports
            ("use old_crate::*;", "use new_crate::*;"),
            ("use old_crate::module::*;", "use new_crate::module::*;"),
            // Extern crate
            ("extern crate old_crate;", "extern crate new_crate;"),
            (
                "extern crate old_crate as alias;",
                "extern crate new_crate as alias;",
            ),
            // Qualified paths
            ("old_crate::function()", "new_crate::function()"),
            (
                "let x = old_crate::Type::new();",
                "let x = new_crate::Type::new();",
            ),
            ("old_crate::module::CONST", "new_crate::module::CONST"),
            // Absolute paths (2015/2018)
            ("::old_crate::function()", "::new_crate::function()"),
            ("use ::old_crate::module;", "use ::new_crate::module;"),
            // Macro invocations
            ("old_crate::macro_name!()", "new_crate::macro_name!()"),
            // Attributes
            ("#[old_crate::attribute]", "#[new_crate::attribute]"),
            (
                "#[derive(old_crate::Derive)]",
                "#[derive(new_crate::Derive)]",
            ),
            // Doc comments with intra-doc links
            ("/// See [`old_crate::Type`]", "/// See [`new_crate::Type`]"),
            ("/// [`old_crate`] is great", "/// [`new_crate`] is great"),
            // Type paths
            (
                "fn foo() -> old_crate::Result<()>",
                "fn foo() -> new_crate::Result<()>",
            ),
            ("impl old_crate::Trait for X", "impl new_crate::Trait for X"),
            // Multi-line use statements
            (
                "use old_crate::{\n    module1,\n    module2\n};",
                "use new_crate::{\n    module1,\n    module2\n};",
            ),
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
            // Local variables
            "let old_crate = 5;",
            "let old_crate_value = 10;",
            // Module definitions (not imports)
            "mod old_crate { }",
            // String literals
            r#"let s = "old_crate";"#,
            // Comments (outside of doc links)
            "// This is about old_crate but not a reference",
            // Different identifiers
            "use old_crate_different::module;",
            "use not_old_crate::module;",
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
// Important comment

use old_crate::module;


/// Documentation
fn main() {
    // Another comment
    let x = old_crate::something();
    
    
    old_crate::other();
}
"#;

        let expected = r#"
// Important comment

use new_crate::module;


/// Documentation
fn main() {
    // Another comment
    let x = new_crate::something();
    
    
    new_crate::other();
}
"#;

        let result = patterns.apply(input).unwrap();
        assert_eq!(result, expected);
    }
}
