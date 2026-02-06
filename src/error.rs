//! Error types for cargo-rename operations.
//!
//! All operations return `Result<T>` which is an alias for `std::result::Result<T, RenameError>`.
//! Errors are designed to provide actionable information to users without exposing internal details.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during package rename operations.
///
/// This enum covers:
/// - **User errors**: Invalid names, missing packages, dirty workspace
/// - **System errors**: I/O failures, permission issues
/// - **Parse errors**: Invalid TOML, regex compilation failures
/// - **Logic errors**: Conflicts, verification failures
#[derive(Debug, Error)]
pub enum RenameError {
    /// The specified package does not exist in the workspace.
    ///
    /// Returned by preflight checks when `old_name` cannot be found in `cargo metadata`.
    #[error("Package '{0}' not found")]
    PackageNotFound(String),

    /// Target directory already exists, preventing move operation.
    ///
    /// Returned when `--move <dir>` specifies a path that already exists on disk.
    /// This prevents accidental overwrites.
    #[error("Target directory already exists: {0}")]
    DirectoryExists(PathBuf),

    /// Package name or directory path violates Cargo naming rules.
    ///
    /// Contains the invalid name and a human-readable explanation.
    /// See `verify::rules` for validation logic.
    #[error("Invalid package name '{0}': {1}")]
    InvalidName(String, String),

    /// Package name or directory path violates Cargo naming rules.
    ///
    /// Contains the invalid name and a human-readable explanation.
    /// See `verify::rules` for validation logic.
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Post-rename workspace verification failed.
    ///
    /// Returned when `cargo metadata` cannot parse the workspace after rename.
    /// This indicates the rename may have corrupted workspace structure.
    #[error("Workspace verification failed: {0}")]
    VerificationFailed(String),

    /// Transaction rollback encountered errors.
    ///
    /// Returned when attempting to undo changes after a failed commit.
    /// Contains description of what failed during rollback.
    #[error("Rollback failed: {0}")]
    RollbackFailed(String),

    /// Git workspace has uncommitted changes.
    ///
    /// Returned by preflight checks unless `--allow-dirty` is specified.
    /// Prevents accidental loss of work.
    #[error("Workspace has uncommitted changes")]
    DirtyWorkspace,

    /// User declined confirmation prompt.
    ///
    /// Returned when interactive confirmation is rejected (not an error condition,
    /// but uses the error path for control flow).
    #[error("Operation cancelled by user")]
    Cancelled,

    /// File system operation failed.
    ///
    /// Wraps `std::io::Error` from file read/write/move operations.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// TOML parsing or serialization failed.
    ///
    /// Returned when `Cargo.toml` cannot be parsed or has invalid structure.
    #[error("TOML error: {0}")]
    Toml(#[from] toml_edit::TomlError),

    /// Cargo metadata command failed.
    ///
    /// Returned when `cargo metadata` cannot execute or returns invalid data.
    #[error("Metadata error: {0}")]
    Metadata(#[from] cargo_metadata::Error),

    /// Regex compilation failed.
    ///
    /// Returned when source code rewrite patterns cannot be compiled.
    /// This should never happen with hardcoded patterns (indicates a bug).
    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    /// Catch-all for unexpected errors.
    ///
    /// Wraps `anyhow::Error` for errors that don't fit other categories.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Result type alias for cargo-rename operations.
///
/// Equivalent to `std::result::Result<T, RenameError>`.
pub type Result<T> = std::result::Result<T, RenameError>;
