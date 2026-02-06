use crate::error::Result;
use crate::ops::transaction::Transaction;
use std::fs;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Value, value};

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
    let content = fs::read_to_string(root_path)?;
    let mut doc: DocumentMut = content.parse()?;
    let mut changed = false;

    if let Some(workspace) = doc.get_mut("workspace").and_then(|w| w.as_table_mut()) {
        // Update workspace.members first (if needed)
        if should_update_members
            && let Some(members) = workspace.get_mut("members").and_then(|m| m.as_array_mut())
        {
            let root_dir = root_path.parent().unwrap();

            let old_rel = pathdiff::diff_paths(old_dir, root_dir)
                .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;
            let new_rel = pathdiff::diff_paths(new_dir, root_dir)
                .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;

            let old_str = old_rel.to_string_lossy().replace('\\', "/");
            let new_str = new_rel.to_string_lossy().replace('\\', "/");

            for i in 0..members.len() {
                if let Some(member_path) = members.get(i).and_then(|v| v.as_str()) {
                    let normalized_member = member_path.replace('\\', "/");

                    if normalized_member == old_str {
                        members.replace(i, &new_str);
                        changed = true;
                        log::info!("Updated workspace.members: {} → {}", old_str, new_str);
                        break;
                    }
                }
            }
        }

        // Update workspace.dependencies
        if name_changed || path_changed {
            if let Some(deps) = workspace
                .get_mut("dependencies")
                .and_then(|d| d.as_table_mut())
            {
                let dep_keys: Vec<String> = deps.iter().map(|(k, _)| k.to_string()).collect();

                for dep_key in dep_keys {
                    if dep_key != old_name {
                        continue;
                    }

                    if let Some(dep_value) = deps.get(&dep_key).cloned() {
                        let mut new_value = dep_value.clone();
                        let mut needs_update = false;

                        match &dep_value {
                            Item::Value(v) if v.is_inline_table() => {
                                if let Some(inline) = v.as_inline_table() {
                                    let mut new_inline = inline.clone();

                                    if old_dir != new_dir && inline.contains_key("path") {
                                        let rel_path = pathdiff::diff_paths(
                                            new_dir,
                                            root_path.parent().unwrap(),
                                        )
                                        .ok_or_else(|| {
                                            anyhow::anyhow!("Failed to calculate path")
                                        })?;
                                        new_inline.insert(
                                            "path",
                                            rel_path.to_string_lossy().as_ref().into(),
                                        );
                                        needs_update = true;
                                    }

                                    if needs_update {
                                        new_value = value(new_inline);
                                    }
                                }
                            }
                            Item::Table(table) => {
                                let mut new_table = table.clone();

                                if old_dir != new_dir && table.contains_key("path") {
                                    let rel_path =
                                        pathdiff::diff_paths(new_dir, root_path.parent().unwrap())
                                            .ok_or_else(|| {
                                                anyhow::anyhow!("Failed to calculate path")
                                            })?;
                                    new_table
                                        .insert("path", value(rel_path.to_string_lossy().as_ref()));
                                    needs_update = true;
                                }

                                if needs_update {
                                    new_value = Item::Table(new_table);
                                }
                            }
                            _ => {}
                        }

                        // PRESERVE POSITION: Handle name change and value update separately
                        if old_name != new_name {
                            // Need to rename the key while preserving position
                            // toml_edit doesn't have a direct "rename" method, so we use a workaround

                            // Get all keys in order
                            let all_keys: Vec<String> =
                                deps.iter().map(|(k, _)| k.to_string()).collect();
                            let key_position = all_keys.iter().position(|k| k == old_name).unwrap();

                            // Create a new table with the same entries in the same order
                            let mut new_deps = toml_edit::Table::new();
                            for (i, key) in all_keys.iter().enumerate() {
                                if i == key_position {
                                    // Insert with new name at the original position
                                    new_deps.insert(new_name, new_value.clone());
                                } else {
                                    // Copy other entries as-is
                                    if let Some(val) = deps.get(key) {
                                        new_deps.insert(key, val.clone());
                                    }
                                }
                            }

                            // Replace the entire dependencies table
                            *deps = new_deps;
                            changed = true;
                            log::info!(
                                "Updated workspace.dependencies: {} → {} (position preserved)",
                                old_name,
                                new_name
                            );
                        } else if needs_update {
                            // Only the value changed (path update), not the name
                            deps.insert(&dep_key, new_value);
                            changed = true;
                            log::info!("Updated workspace.dependencies path for: {}", old_name);
                        }
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

/// Updates [workspace.dependencies] entries when renaming a package
///
/// This fixes the bug where workspace.dependencies were not updated during rename.
pub fn update_workspace_dependencies(
    root_path: &Path,
    old_name: &str,
    new_name: &str,
    old_dir: &Path,
    new_dir: &Path,
    txn: &mut Transaction,
) -> Result<()> {
    let content = fs::read_to_string(root_path)?;
    let mut doc: DocumentMut = content.parse()?;
    let mut changed = false;

    if let Some(workspace) = doc.get_mut("workspace").and_then(|w| w.as_table_mut())
        && let Some(deps) = workspace
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
    {
        // Collect keys to avoid borrow checker issues
        let dep_keys: Vec<String> = deps.iter().map(|(k, _)| k.to_string()).collect();

        for dep_key in dep_keys {
            if dep_key != old_name {
                continue;
            }

            if let Some(dep_value) = deps.get(&dep_key).cloned() {
                let mut new_value = dep_value.clone();
                let mut needs_update = false;

                match &dep_value {
                    Item::Value(v) if v.is_inline_table() => {
                        if let Some(inline) = v.as_inline_table() {
                            let mut new_inline = inline.clone();

                            // Update path if changed
                            if old_dir != new_dir && inline.contains_key("path") {
                                let rel_path =
                                    pathdiff::diff_paths(new_dir, root_path.parent().unwrap())
                                        .ok_or_else(|| {
                                            anyhow::anyhow!("Failed to calculate path")
                                        })?;
                                new_inline
                                    .insert("path", rel_path.to_string_lossy().as_ref().into());
                                needs_update = true;
                            }

                            if needs_update {
                                new_value = value(new_inline);
                            }
                        }
                    }
                    Item::Table(table) => {
                        let mut new_table = table.clone();

                        // Update path if changed
                        if old_dir != new_dir && table.contains_key("path") {
                            let rel_path =
                                pathdiff::diff_paths(new_dir, root_path.parent().unwrap())
                                    .ok_or_else(|| anyhow::anyhow!("Failed to calculate path"))?;
                            new_table.insert("path", value(rel_path.to_string_lossy().as_ref()));
                            needs_update = true;
                        }

                        if needs_update {
                            new_value = Item::Table(new_table);
                        }
                    }
                    _ => {}
                }

                // Rename the key if package name changed
                if needs_update || old_name != new_name {
                    deps.remove(&dep_key);
                    deps.insert(new_name, new_value);
                    changed = true;
                }
            }
        }
    }

    if changed {
        txn.update_file(root_path.to_path_buf(), doc.to_string())?;
    }

    Ok(())
}

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

    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(deps_item) = doc.get_mut(section) {
            let Some(deps) = deps_item.as_table_mut() else {
                continue;
            };

            let dep_keys: Vec<String> = deps.iter().map(|(k, _)| k.to_string()).collect();

            for dep_key in dep_keys {
                if let Some(dep_value) = deps.get(&dep_key).cloned() {
                    let mut is_target = dep_key == old_name;
                    let mut new_value = dep_value.clone();
                    let mut needs_update = false;

                    // Check based on value type
                    match &dep_value {
                        Item::Value(v) if v.is_inline_table() => {
                            if let Some(inline) = v.as_inline_table() {
                                // Check package field
                                if let Some(pkg) = inline.get("package")
                                    && pkg.as_str() == Some(old_name)
                                {
                                    is_target = true;
                                    if name_changed {
                                        // PRESERVE FORMATTING: only update the value, not recreate
                                        let mut new_inline = inline.clone();
                                        new_inline.insert("package", new_name.into());
                                        new_value = value(new_inline);
                                        needs_update = true;
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
                                    updated_inline
                                        .insert("path", rel_path.to_string_lossy().as_ref().into());
                                    new_value = value(updated_inline);
                                    needs_update = true;
                                }
                            }
                        }
                        Item::Table(table) => {
                            // Similar logic for tables
                            if let Some(pkg) = table.get("package")
                                && pkg.as_str() == Some(old_name)
                            {
                                is_target = true;
                                if name_changed {
                                    let mut new_table = table.clone();
                                    new_table.insert("package", value(new_name));
                                    new_value = Item::Table(new_table);
                                    needs_update = true;
                                }
                            }

                            if is_target && path_changed && table.contains_key("path") {
                                let rel_path = pathdiff::diff_paths(new_dir, manifest_dir)
                                    .ok_or_else(|| anyhow::anyhow!("Failed to calculate path"))?;

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

                    let new_dep_key =
                        if is_target && name_changed && dep_key == old_name && !has_package_field {
                            new_name.to_string()
                        } else {
                            dep_key.clone()
                        };

                    if needs_update || new_dep_key != dep_key {
                        if new_dep_key != dep_key {
                            // Name is changing - preserve position
                            let all_keys: Vec<String> =
                                deps.iter().map(|(k, _)| k.to_string()).collect();
                            let key_position = all_keys.iter().position(|k| k == &dep_key).unwrap();

                            let mut new_deps = toml_edit::Table::new();
                            for (i, key) in all_keys.iter().enumerate() {
                                if i == key_position {
                                    new_deps.insert(&new_dep_key, new_value.clone());
                                } else {
                                    if let Some(val) = deps.get(key) {
                                        new_deps.insert(key, val.clone());
                                    }
                                }
                            }

                            *deps = new_deps;
                            changed = true;
                            log::debug!(
                                "Renamed dependency {} → {} (position preserved)",
                                dep_key,
                                new_dep_key
                            );
                        } else {
                            // Only value changed, not the key
                            deps.insert(&dep_key, new_value);
                            changed = true;
                            log::debug!("Updated dependency value for: {}", dep_key);
                        }
                    }
                }
            }
        }
    }

    if changed {
        // toml_edit preserves formatting by default when using to_string()
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

    if let Some(workspace) = doc.get_mut("workspace").and_then(|w| w.as_table_mut())
        && let Some(members) = workspace.get_mut("members").and_then(|m| m.as_array_mut())
    {
        let root_dir = root_path.parent().unwrap();

        let old_rel = pathdiff::diff_paths(old_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;
        let new_rel = pathdiff::diff_paths(new_dir, root_dir)
            .ok_or_else(|| anyhow::anyhow!("Failed to calc path"))?;

        // Normalize to forward slashes
        let old_str = old_rel.to_string_lossy().replace('\\', "/");
        let new_str = new_rel.to_string_lossy().replace('\\', "/");

        log::debug!(
            "Looking for workspace member: '{}' to update to '{}'",
            old_str,
            new_str
        );

        for i in 0..members.len() {
            if let Some(member_path) = members.get(i).and_then(|v| v.as_str()) {
                let normalized_member = member_path.replace('\\', "/");

                if normalized_member == old_str {
                    log::debug!("Found matching member at index {}", i);
                    members.replace(i, &new_str);
                    changed = true;
                    log::info!("Updated workspace.members: {} → {}", old_str, new_str);
                    break;
                }
            }
        }

        if !changed {
            log::warn!(
                "Could not find exact match for '{}' in workspace.members",
                old_str
            );
            log::debug!(
                "Available members: {:?}",
                members
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
            );

            // Check if it's covered by a glob pattern
            for member_path in members.iter().filter_map(|v| v.as_str()) {
                let normalized = member_path.replace('\\', "/");
                if normalized.contains('*') {
                    let prefix = normalized.trim_end_matches('*').trim_end_matches('/');
                    if old_str.starts_with(prefix) && new_str.starts_with(prefix) {
                        log::info!("Member is covered by glob pattern: {}", normalized);
                        log::info!("No workspace.members update needed");
                        return Ok(()); // Early return - no update needed
                    }
                }
            }

            log::warn!("No glob pattern covers this member either - workspace may be broken!");
        }
    }

    // CRITICAL: Always try to update the file if we made changes
    if changed {
        log::debug!(
            "Writing updated workspace.members to {}",
            root_path.display()
        );
        txn.update_file(root_path.to_path_buf(), doc.to_string())?;
        log::debug!("Workspace.members update queued in transaction");
    } else {
        log::warn!("No changes made to workspace.members");
    }

    Ok(())
}
