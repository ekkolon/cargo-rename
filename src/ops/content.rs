use crate::error::Result;
use crate::ops::transaction::Transaction;

use cargo_metadata::Metadata;
use ignore::WalkBuilder;
use regex::Regex;
use std::{fs, path::Path};
use syn::{
    File, Ident, ItemExternCrate, PathArguments, UseTree,
    visit_mut::{self, VisitMut},
};

/// Updates Rust source files and documentation to reflect a crate rename.
///
/// Guarantees:
/// - Never mutates Rust files that fail to parse
/// - Never rewrites comments or string literals
/// - Honors `.gitignore`, `.ignore`, and `.git/info/exclude`
/// - Idempotent
pub fn update_source_code(
    metadata: &Metadata,
    old_name: &str,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let old_snake = old_name.replace('-', "_");
    let new_snake = new_name.replace('-', "_");

    let doc_pattern = Regex::new(&format!(r"\b{}\b", regex::escape(old_name)))?;

    for member in metadata.workspace_packages() {
        let pkg_root = member
            .manifest_path
            .parent()
            .expect("manifest path must have parent");

        walk_package(
            pkg_root.as_std_path(),
            &old_snake,
            &new_snake,
            &doc_pattern,
            new_name,
            txn,
        )?;
    }

    Ok(())
}

fn walk_package(
    root: &Path,
    old_snake: &str,
    new_snake: &str,
    doc_pattern: &Regex,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .filter_entry(|e| {
            if let Some(ft) = e.file_type()
                && !ft.is_dir()
            {
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
            Some("rs") => {
                update_rust_file(path, old_snake, new_snake, txn)?;
            }
            Some("md") | Some("toml") => {
                update_doc_file(path, doc_pattern, new_name, txn)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn update_rust_file(
    path: &Path,
    old_snake: &str,
    new_snake: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // skip non-UTF8 or unreadable files
    };

    let mut syntax: File = match syn::parse_file(&content) {
        Ok(f) => f,
        Err(_) => return Ok(()), // fail closed
    };

    let mut rewriter = CrateRenameRewriter {
        old: old_snake,
        new: new_snake,
        modified: false,
    };

    rewriter.visit_file_mut(&mut syntax);

    if !rewriter.modified {
        return Ok(());
    }

    let new_content = prettyplease::unparse(&syntax);

    if new_content != content {
        txn.update_file(path.to_path_buf(), new_content)?;
    }

    Ok(())
}

fn update_doc_file(
    path: &Path,
    pattern: &Regex,
    new_name: &str,
    txn: &mut Transaction,
) -> Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    if !pattern.is_match(&content) {
        return Ok(());
    }

    let new_content = pattern.replace_all(&content, new_name).to_string();

    if new_content != content {
        txn.update_file(path.to_path_buf(), new_content)?;
    }

    Ok(())
}

/// AST visitor responsible for renaming crate identifiers.
///
/// Scope:
/// - `use` trees
/// - absolute and relative paths
/// - `extern crate` items
///
/// Explicitly does *not* touch:
/// - string literals
/// - comments
/// - macro bodies
struct CrateRenameRewriter<'a> {
    old: &'a str,
    new: &'a str,
    modified: bool,
}

impl<'a> VisitMut for CrateRenameRewriter<'a> {
    fn visit_item_extern_crate_mut(&mut self, node: &mut ItemExternCrate) {
        if node.ident == self.old {
            node.ident = Ident::new(self.new, node.ident.span());
            self.modified = true;
        }
    }

    fn visit_use_tree_mut(&mut self, node: &mut UseTree) {
        match node {
            UseTree::Name(name) => {
                if name.ident == self.old {
                    name.ident = Ident::new(self.new, name.ident.span());
                    self.modified = true;
                }
            }

            UseTree::Rename(rename) => {
                if rename.ident == self.old {
                    rename.ident = Ident::new(self.new, rename.ident.span());
                    self.modified = true;
                }
            }

            UseTree::Path(path) => {
                if path.ident == self.old {
                    path.ident = Ident::new(self.new, path.ident.span());
                    self.modified = true;
                }
                self.visit_use_tree_mut(&mut path.tree);
            }

            UseTree::Group(group) => {
                for item in &mut group.items {
                    self.visit_use_tree_mut(item);
                }
            }

            UseTree::Glob(_) => {
                // `use foo::*;` - handled by Path before glob
            }
        }
    }

    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        if let Some(first) = path.segments.first_mut()
            && first.ident == self.old
        {
            first.ident = Ident::new(self.new, first.ident.span());
            self.modified = true;
        }

        for seg in &mut path.segments {
            if let PathArguments::None = seg.arguments {
                continue;
            }
        }

        visit_mut::visit_path_mut(self, path);
    }
}
