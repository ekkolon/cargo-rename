use crate::error::Result;
use crate::ops::transaction::Transaction;
use cargo_metadata::Metadata;
use regex::Regex;
use std::fs;

/// TODO (ekkolon) - use ignore crate to exclude globs specified in `.gitignore`
pub fn update_source_code(
    metadata: &Metadata,
    old_name: &str,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let old_snake = old_name.replace('-', "_");
    let new_snake = new_name.replace('-', "_");

    let rust_patterns = build_rust_patterns(&old_snake)?;
    let doc_pattern = Regex::new(&format!(r"\b{}\b", regex::escape(old_name)))?;

    for member in metadata.workspace_packages() {
        let pkg_root = member.manifest_path.parent().unwrap();

        for entry in walkdir::WalkDir::new(pkg_root)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !matches!(name.as_ref(), "target" | ".git" | "node_modules")
            })
            .filter_map(walkdir::Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();

            match path.extension().and_then(|s| s.to_str()) {
                Some("rs") => {
                    update_rust_file(path, &rust_patterns, &old_snake, &new_snake, txn)?;
                }
                Some("md") | Some("toml") => {
                    update_doc_file(path, &doc_pattern, new_name, txn)?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn build_rust_patterns(old_snake: &str) -> Result<Vec<Regex>> {
    let patterns = vec![
        format!(r"\buse\s+{}\b", regex::escape(old_snake)),
        format!(r"\bextern\s+crate\s+{}\b", regex::escape(old_snake)),
        format!(r"\b{}::", regex::escape(old_snake)),
    ];

    patterns
        .into_iter()
        .map(|p| Regex::new(&p).map_err(Into::into))
        .collect()
}

fn update_rust_file(
    path: &std::path::Path,
    patterns: &[Regex],
    old_snake: &str,
    new_snake: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let mut new_content = content.clone();
    let mut changed = false;

    for pattern in patterns {
        if pattern.is_match(&new_content) {
            new_content = pattern
                .replace_all(&new_content, |caps: &regex::Captures| {
                    caps[0].replace(old_snake, new_snake)
                })
                .to_string();
            changed = true;
        }
    }

    if changed {
        txn.update_file(path.to_path_buf(), new_content)?;
    }

    Ok(())
}

fn update_doc_file(
    path: &std::path::Path,
    pattern: &Regex,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let content = fs::read_to_string(path)?;

    if pattern.is_match(&content) {
        let new_content = pattern.replace_all(&content, new_name).to_string();
        txn.update_file(path.to_path_buf(), new_content)?;
    }

    Ok(())
}
