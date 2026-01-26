use crate::error::Result;
use crate::ops::transaction::Transaction;
use std::fs;
use std::path::Path;
use toml_edit::{value, DocumentMut, Item, Value};

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
    let mut doc: DocumentMut = content.parse()?;
    let mut changed = false;
    let manifest_dir = manifest_path.parent().unwrap();

    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(deps_item) = doc.get_mut(section) {
            if let Some(deps) = deps_item.as_table_mut() {
                let dep_keys: Vec<String> = deps.iter().map(|(k, _)| k.to_string()).collect();

                for dep_key in dep_keys {
                    if let Some(dep_value) = deps.get(&dep_key).cloned() {
                        let mut is_target = dep_key == old_name;
                        let mut new_value = dep_value.clone();
                        let mut needs_update = false;

                        // Check and update based on value type
                        match &dep_value {
                            Item::Value(v) if v.is_inline_table() => {
                                if let Some(inline) = v.as_inline_table() {
                                    // Check package field
                                    if let Some(pkg) = inline.get("package") {
                                        if pkg.as_str() == Some(old_name) {
                                            is_target = true;
                                            if name_changed {
                                                let mut new_inline = inline.clone();
                                                new_inline.insert("package", new_name.into());
                                                new_value = value(new_inline);
                                                needs_update = true;
                                            }
                                        }
                                    }

                                    // Update path if needed
                                    if is_target && path_changed && inline.contains_key("path") {
                                        let rel_path = pathdiff::diff_paths(new_dir, manifest_dir)
                                            .ok_or_else(|| {
                                                anyhow::anyhow!("Failed to calculate path")
                                            })?;

                                        let inline_to_update = new_value
                                            .as_value()
                                            .and_then(|v| v.as_inline_table())
                                            .cloned()
                                            .unwrap_or_else(|| inline.clone());

                                        let mut updated_inline = inline_to_update;
                                        updated_inline.insert(
                                            "path",
                                            rel_path.to_string_lossy().as_ref().into(),
                                        );
                                        new_value = value(updated_inline);
                                        needs_update = true;
                                    }
                                }
                            }
                            Item::Table(table) => {
                                if let Some(pkg) = table.get("package") {
                                    if pkg.as_str() == Some(old_name) {
                                        is_target = true;
                                        if name_changed {
                                            let mut new_table = table.clone();
                                            new_table.insert("package", value(new_name));
                                            new_value = Item::Table(new_table);
                                            needs_update = true;
                                        }
                                    }
                                }

                                if is_target && path_changed && table.contains_key("path") {
                                    let rel_path = pathdiff::diff_paths(new_dir, manifest_dir)
                                        .ok_or_else(|| {
                                            anyhow::anyhow!("Failed to calculate path")
                                        })?;

                                    let table_to_update = new_value
                                        .as_table()
                                        .cloned()
                                        .unwrap_or_else(|| table.clone());

                                    let mut updated_table = table_to_update;
                                    updated_table
                                        .insert("path", value(rel_path.to_string_lossy().as_ref()));
                                    new_value = Item::Table(updated_table);
                                    needs_update = true;
                                }
                            }
                            _ => {}
                        }

                        // Determine if key needs to change
                        let has_package_field = match &new_value {
                            Item::Value(v) if v.is_inline_table() => v
                                .as_inline_table()
                                .is_some_and(|t| t.contains_key("package")),
                            Item::Table(t) => t.contains_key("package"),
                            _ => false,
                        };

                        let new_dep_key = if is_target
                            && name_changed
                            && dep_key == old_name
                            && !has_package_field
                        {
                            new_name.to_string()
                        } else {
                            dep_key.clone()
                        };

                        if needs_update || new_dep_key != dep_key {
                            deps.remove(&dep_key);
                            deps.insert(&new_dep_key, new_value);
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    if changed {
        txn.update_file(manifest_path.to_path_buf(), doc.to_string())?;
    }

    Ok(())
}

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

pub fn update_workspace_members(
    root_path: &Path,
    old_dir: &Path,
    new_dir: &Path,
    txn: &mut Transaction,
) -> Result<()> {
    let content = fs::read_to_string(root_path)?;
    let mut doc: DocumentMut = content.parse()?;
    let mut changed = false;

    if let Some(workspace) = doc.get_mut("workspace").and_then(|w| w.as_table_mut()) {
        if let Some(members) = workspace.get_mut("members").and_then(|m| m.as_array_mut()) {
            let root_dir = root_path.parent().unwrap();

            let old_rel = pathdiff::diff_paths(old_dir, root_dir)
                .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;
            let new_rel = pathdiff::diff_paths(new_dir, root_dir)
                .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;

            let old_str = old_rel.to_string_lossy();
            let new_str = new_rel.to_string_lossy();

            for i in 0..members.len() {
                if let Some(member_path) = members.get(i).and_then(|v| v.as_str()) {
                    if member_path == old_str.as_ref() {
                        members.replace(i, new_str.as_ref());
                        changed = true;
                        break;
                    }
                }
            }
        }
    }

    if changed {
        txn.update_file(root_path.to_path_buf(), doc.to_string())?;
    }

    Ok(())
}
