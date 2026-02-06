//! Package manifest updates.
//!
//! Updates the `[package]` section of a crate's `Cargo.toml`.

use crate::error::Result;
use crate::fs::transaction::Transaction;
use std::fs;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Value};

/// Updates package name in `Cargo.toml`.
///
/// Modifies only the `name` field, preserving formatting and comments.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_update_package_name() {
        let temp = TempDir::new().unwrap();
        let manifest = temp.path().join("Cargo.toml");

        fs::write(
            &manifest,
            "[package]\nname = \"old-name\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let mut txn = Transaction::new(false);
        update_package_name(&manifest, "new-name", &mut txn).unwrap();
        txn.commit().unwrap();

        let result = fs::read_to_string(&manifest).unwrap();
        assert!(result.contains("name = \"new-name\""));
        assert!(result.contains("version = \"0.1.0\""));
    }

    #[test]
    fn test_preserves_comments() {
        let temp = TempDir::new().unwrap();
        let manifest = temp.path().join("Cargo.toml");

        let input = r#"[package]
# Important: This is the package name
name = "old-name"
version = "0.1.0"
"#;
        fs::write(&manifest, input).unwrap();

        let mut txn = Transaction::new(false);
        update_package_name(&manifest, "new-name", &mut txn).unwrap();
        txn.commit().unwrap();

        let result = fs::read_to_string(&manifest).unwrap();
        assert!(result.contains("# Important"));
        assert!(result.contains("name = \"new-name\""));
    }
}
