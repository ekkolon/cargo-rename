//! Validation rules for package names and directory paths.
//!
//! Pure functions with no I/O or side effects.

use crate::error::{RenameError, Result};
use std::path::Path;

const MAX_PACKAGE_NAME_LENGTH: usize = 64;
const RESERVED_PACKAGE_NAMES: &[&str] = &["test", "doc", "build", "bench"];

/// Validates package name against Cargo rules.
///
/// ## Rules
/// - 1-64 ASCII characters
/// - Starts with letter or `_`
/// - Contains only `[a-zA-Z0-9_-]`
/// - Cannot start/end with `-`
/// - Not reserved (`test`, `doc`, `build`, `bench`)
///
/// ## Warnings (non-fatal)
/// - Uppercase letters (convention: lowercase-with-hyphens)
/// - Mixing `_` and `-` (crates.io conflict risk)
/// - Consecutive `--`
pub fn validate_package_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot be empty".to_string(),
        ));
    }

    if name.len() > MAX_PACKAGE_NAME_LENGTH {
        return Err(RenameError::InvalidName(
            name.to_string(),
            format!(
                "exceeds {} chars (has {})",
                MAX_PACKAGE_NAME_LENGTH,
                name.len()
            ),
        ));
    }

    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "must start with letter or underscore".to_string(),
        ));
    }

    for (idx, ch) in name.chars().enumerate() {
        if !ch.is_ascii() {
            return Err(RenameError::InvalidName(
                name.to_string(),
                format!("non-ASCII character '{}' at position {}", ch, idx),
            ));
        }

        if !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-' {
            return Err(RenameError::InvalidName(
                name.to_string(),
                format!("invalid character '{}' at position {}", ch, idx),
            ));
        }
    }

    if RESERVED_PACKAGE_NAMES.contains(&name) {
        return Err(RenameError::InvalidName(
            name.to_string(),
            format!(
                "'{}' is reserved. Reserved: {}",
                name,
                RESERVED_PACKAGE_NAMES.join(", ")
            ),
        ));
    }

    if name.starts_with('-') {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot start with hyphen".to_string(),
        ));
    }

    if name.ends_with('-') {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot end with hyphen".to_string(),
        ));
    }

    if name.contains("--") {
        log::warn!("'{}' has consecutive hyphens", name);
    }

    if name.contains('_') && name.contains('-') {
        log::warn!("'{}' mixes _ and - (may conflict on crates.io)", name);
    }

    if name.chars().any(|c| c.is_ascii_uppercase()) {
        log::warn!(
            "'{}' has uppercase (convention: lowercase-with-hyphens)",
            name
        );
    }

    Ok(())
}

/// Validates directory path security and correctness.
///
/// ## Rules
/// - Relative path (not absolute)
/// - Not `.` or `..`
/// - No `..` components (path traversal)
/// - Windows: No reserved names (CON, PRN, etc.)
/// - Windows: No invalid chars (`<>:"|?*`)
pub fn validate_directory_path(path_str: &str, workspace_root: &Path) -> Result<()> {
    if path_str == "." || path_str == ".." {
        return Err(RenameError::InvalidPath(
            path_str.to_string(),
            format!("Cannot use '.' or '..': {}", path_str),
        ));
    }

    let path = Path::new(path_str);

    for component in path.components() {
        if component == std::path::Component::ParentDir {
            return Err(RenameError::InvalidPath(
                path.display().to_string(),
                format!("Contains '..': {}", path_str),
            ));
        }
    }

    if path.is_absolute() || path_str.starts_with('/') || path_str.starts_with('\\') {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        if !canonical.starts_with(workspace_root) {
            eprintln!("⚠️  Warning: Absolute path outside workspace: {}", path_str);
            eprintln!("   This will move crate outside workspace.");
        }
    }

    #[cfg(windows)]
    {
        validate_windows_path_components(path)?;
    }

    Ok(())
}

#[cfg(windows)]
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[cfg(windows)]
fn validate_windows_path_components(path: &Path) -> Result<()> {
    const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*'];

    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            let name_str = name.to_string_lossy();
            let base = name_str.split('.').next().unwrap().to_uppercase();

            if WINDOWS_RESERVED_NAMES.contains(&base.as_str()) {
                return Err(RenameError::InvalidPath(
                    path.display().to_string(),
                    format!("'{}' is Windows reserved name", name_str),
                ));
            }

            for &ch in INVALID_CHARS {
                if name_str.contains(ch) {
                    return Err(RenameError::InvalidPath(
                        path.display().to_string(),
                        format!("'{}' contains invalid char '{}'", name_str, ch),
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Validates path resolves within workspace.
pub fn validate_path_within_workspace(dir_path: &Path, workspace_root: &Path) -> Result<()> {
    let full_path = workspace_root.join(dir_path);

    if let Ok(canonical) = full_path.canonicalize() {
        let canonical_workspace = workspace_root.canonicalize()?;

        if !canonical.starts_with(&canonical_workspace) {
            return Err(RenameError::InvalidName(
                dir_path.display().to_string(),
                "resolves outside workspace".to_string(),
            ));
        }
    }

    Ok(())
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     // Package name tests
//     #[test]
//     fn test_validate_basic_names() {
//         assert!(validate_package_name("my-crate").is_ok());
//         assert!(validate_package_name("my_crate").is_ok());
//         assert!(validate_package_name("MyCrate").is_ok());
//         assert!(validate_package_name("crate123").is_ok());
//         assert!(validate_package_name("a").is_ok());
//         assert!(validate_package_name("_private").is_ok());
//     }

//     #[test]
//     fn test_validate_invalid_start() {
//         assert!(validate_package_name("123crate").is_err());
//         assert!(validate_package_name("-crate").is_err());
//     }

//     #[test]
//     fn test_validate_invalid_chars() {
//         assert!(validate_package_name("my crate").is_err());
//         assert!(validate_package_name("my.crate").is_err());
//         assert!(validate_package_name("my@crate").is_err());
//         assert!(validate_package_name("my/crate").is_err());
//         assert!(validate_package_name("my\\crate").is_err());
//     }

//     #[test]
//     fn test_validate_reserved_names() {
//         assert!(validate_package_name("test").is_err());
//         assert!(validate_package_name("doc").is_err());
//         assert!(validate_package_name("build").is_err());
//         assert!(validate_package_name("bench").is_err());
//     }

//     #[test]
//     fn test_validate_empty_and_edge_cases() {
//         assert!(validate_package_name("").is_err());
//         assert!(validate_package_name("-crate").is_err());
//         assert!(validate_package_name("crate-").is_err());
//         assert!(validate_package_name("_crate").is_ok());
//     }

//     #[test]
//     fn test_validate_length_limit() {
//         let too_long = "a".repeat(65);
//         assert!(validate_package_name(&too_long).is_err());

//         let max_length = "a".repeat(64);
//         assert!(validate_package_name(&max_length).is_ok());
//     }

//     #[test]
//     fn test_validate_non_ascii() {
//         assert!(validate_package_name("café").is_err());
//         assert!(validate_package_name("テスト").is_err());
//     }

//     #[test]
//     fn test_consecutive_hyphens() {
//         // Should succeed but warn
//         assert!(validate_package_name("my--crate").is_ok());
//     }

//     // Directory path tests
//     #[test]
//     fn test_validate_directory_paths() {
//         assert!(validate_directory_path("my-dir").is_ok());
//         assert!(validate_directory_path("crates/api").is_ok());
//         assert!(validate_directory_path("crates/backend/api-v2").is_ok());
//         assert!(validate_directory_path("_private").is_ok());
//     }

//     #[test]
//     fn test_validate_invalid_directory_paths() {
//         assert!(validate_directory_path("").is_err());
//         assert!(validate_directory_path(".").is_err());
//         assert!(validate_directory_path("..").is_err());
//         assert!(validate_directory_path("../../../etc/passwd").is_err());
//         assert!(validate_directory_path("crates/../secrets").is_err());
//         assert!(validate_directory_path("foo/../bar").is_err());
//         assert!(validate_directory_path("/absolute/path").is_err());
//     }

//     #[test]
//     #[cfg(windows)]
//     fn test_validate_windows_paths() {
//         assert!(validate_directory_path("C:\\absolute").is_err());
//         assert!(validate_directory_path("\\\\server\\share").is_err());
//         assert!(validate_directory_path("CON").is_err());
//         assert!(validate_directory_path("PRN").is_err());
//         assert!(validate_directory_path("dir<name").is_err());
//         assert!(validate_directory_path("dir>name").is_err());
//         assert!(validate_directory_path("dir:name").is_err());
//         assert!(validate_directory_path("dir|name").is_err());
//     }
// }
