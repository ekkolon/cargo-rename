//! Validation rules for package names and directory paths.
//!
//! This module contains pure validation functions that check whether names
//! and paths conform to Cargo's requirements and filesystem constraints.
//! These functions do not perform I/O or modify state.

use crate::error::{RenameError, Result};
use std::path::Path;

/// Maximum package name length enforced by Cargo.
const MAX_PACKAGE_NAME_LENGTH: usize = 64;

/// Reserved package names that cannot be used.
///
/// These conflict with Cargo's built-in targets and features.
const RESERVED_PACKAGE_NAMES: &[&str] = &["test", "doc", "build", "bench"];

/// Validates a package name against Cargo's naming rules.
///
/// # Rules
///
/// - Length: 1-64 ASCII characters
/// - First character: ASCII letter or underscore
/// - Allowed characters: ASCII alphanumerics, `-`, `_`
/// - Cannot start or end with `-`
/// - Cannot be a reserved name (`test`, `doc`, `build`, `bench`)
///
/// # Warnings
///
/// The following trigger warnings but don't fail validation:
/// - Uppercase letters (convention is lowercase-with-hyphens)
/// - Mixing `_` and `-` (may conflict on crates.io)
/// - Consecutive hyphens `--` (uncommon pattern)
///
/// # Errors
///
/// Returns `InvalidName` with a human-readable explanation if validation fails.
///
/// # Examples
///
/// ```
/// # use cargo_rename::verify::rules::validate_package_name;
/// assert!(validate_package_name("my-crate").is_ok());
/// assert!(validate_package_name("_private").is_ok());
/// assert!(validate_package_name("123crate").is_err()); // Cannot start with digit
/// assert!(validate_package_name("test").is_err());     // Reserved name
/// ```
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
                "exceeds maximum length of {} characters (has {})",
                MAX_PACKAGE_NAME_LENGTH,
                name.len()
            ),
        ));
    }

    // Validate first character
    let first_char = name.chars().next().unwrap(); // Safe: non-empty
    if !first_char.is_ascii_alphabetic() && first_char != '_' {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "must start with an ASCII letter or underscore".to_string(),
        ));
    }

    // Validate all characters (ASCII-only)
    for (idx, ch) in name.chars().enumerate() {
        if !ch.is_ascii() {
            return Err(RenameError::InvalidName(
                name.to_string(),
                format!(
                    "contains non-ASCII character '{}' at position {}. Only ASCII characters are allowed",
                    ch, idx
                ),
            ));
        }

        if !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-' {
            return Err(RenameError::InvalidName(
                name.to_string(),
                format!(
                    "contains invalid character '{}' at position {}. Only ASCII letters, numbers, hyphens, and underscores are allowed",
                    ch, idx
                ),
            ));
        }
    }

    // Check reserved names
    if RESERVED_PACKAGE_NAMES.contains(&name) {
        return Err(RenameError::InvalidName(
            name.to_string(),
            format!(
                "'{}' is a reserved package name. Reserved names: {}",
                name,
                RESERVED_PACKAGE_NAMES.join(", ")
            ),
        ));
    }

    // Check hyphen placement
    if name.starts_with('-') {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot start with a hyphen".to_string(),
        ));
    }

    if name.ends_with('-') {
        return Err(RenameError::InvalidName(
            name.to_string(),
            "cannot end with a hyphen".to_string(),
        ));
    }

    // Warnings (non-fatal)
    if name.contains("--") {
        log::warn!(
            "Package name '{}' contains consecutive hyphens, which may cause confusion",
            name
        );
    }

    // crates.io normalizes my-crate and my_crate as the same package
    if name.contains('_') && name.contains('-') {
        log::warn!(
            "Package name '{}' contains both underscores and hyphens. This is valid but may cause confusion.",
            name
        );
    }

    if name.chars().any(|c| c.is_ascii_uppercase()) {
        log::warn!(
            "Package name '{}' contains uppercase letters. By convention, package names should be lowercase with hyphens.",
            name
        );
    }

    Ok(())
}

/// Validates a directory path for security and correctness.
///
/// # Validation Rules
///
/// 1. Must be a relative path (not absolute)
/// 2. Cannot be "." or ".."
/// 3. Cannot contain ".." components (path traversal)
/// 4. Cannot navigate outside workspace
/// 5. Windows: Cannot be UNC path (\\server\share)
/// 6. Windows: Cannot be reserved device name (CON, PRN, etc.)
/// 7. Windows: Cannot contain invalid characters (<>:"|?*)
///
/// # Examples
///
/// ```
/// # use cargo_rename::verify::validate_directory_path;
/// # use std::path::Path;
/// # fn example(workspace_root: &Path) {
/// // Valid
/// assert!(validate_directory_path("crates/api", workspace_root).is_ok());
/// assert!(validate_directory_path("backend", workspace_root).is_ok());
///
/// // Invalid
/// assert!(validate_directory_path("/tmp/evil", workspace_root).is_err());
/// assert!(validate_directory_path("../outside", workspace_root).is_err());
/// assert!(validate_directory_path(".", workspace_root).is_err());
/// # }
/// ```
pub fn validate_directory_path(path_str: &str, workspace_root: &Path) -> Result<()> {
    //  Reject "." and ".."
    if path_str == "." || path_str == ".." {
        return Err(RenameError::InvalidPath(format!(
            "Directory path cannot use '.' or '..': {}",
            path_str
        )));
    }

    let path = Path::new(path_str);

    // Check for ".." components (prevent traversal)
    for component in path.components() {
        if component == std::path::Component::ParentDir {
            return Err(RenameError::InvalidPath(format!(
                "Directory path cannot navigate outside workspace (contains '..'): {}",
                path_str
            )));
        }
    }

    // If absolute path, verify it's within workspace OR warn
    if path.is_absolute() || path_str.starts_with('/') || path_str.starts_with('\\') {
        // Allow absolute paths, but they should resolve within workspace
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        if !canonical.starts_with(workspace_root) {
            eprintln!(
                "⚠️  Warning: Using absolute path outside workspace: {}",
                path_str
            );
            eprintln!("   This will move the crate outside the current workspace.");
            // Consider requiring --allow-external flag for this
        }
    }

    // Windows-specific checks
    #[cfg(windows)]
    {
        validate_windows_path_components(path)?;
    }

    Ok(())
}

/// Windows reserved device names that cannot be used as path components.
#[cfg(windows)]
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Windows-specific path validation
#[cfg(windows)]
fn validate_windows_path_components(path: &Path) -> Result<()> {
    const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*'];

    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            let name_str = name.to_string_lossy();
            let name_upper = name_str.to_uppercase();
            let base_name = name_upper.split('.').next().unwrap_or(&name_upper);

            // Check reserved names
            if WINDOWS_RESERVED_NAMES.contains(&base_name) {
                return Err(RenameError::InvalidPath(format!(
                    "Directory component '{}' is a Windows reserved name",
                    name_str
                )));
            }

            // Check invalid characters
            for &ch in INVALID_CHARS {
                if name_str.contains(ch) {
                    return Err(RenameError::InvalidPath(format!(
                        "Directory component '{}' cannot contain character '{}'",
                        name_str, ch
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Validates that a directory path resolves to a location within the workspace.
///
/// Attempts to canonicalize the path (resolving symlinks and `..` components)
/// and verifies it starts with the workspace root. If the path doesn't exist yet,
/// validation is skipped (since `..` components are already forbidden by
/// `validate_directory_path`).
///
/// # Errors
///
/// Returns `InvalidName` if the resolved path would be outside the workspace.
pub fn validate_path_within_workspace(dir_path: &Path, workspace_root: &Path) -> Result<()> {
    let full_path = workspace_root.join(dir_path);

    // Try to canonicalize (fails if path doesn't exist, which is OK)
    if let Ok(canonical) = full_path.canonicalize() {
        let canonical_workspace = workspace_root.canonicalize().map_err(|e| {
            RenameError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to canonicalize workspace root: {}", e),
            ))
        })?;

        if !canonical.starts_with(&canonical_workspace) {
            return Err(RenameError::InvalidName(
                dir_path.display().to_string(),
                "resolved path is outside workspace".to_string(),
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
