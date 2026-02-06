//! Error types for cargo-rename.
//!
//! All operations return `Result<T>` which aliases `Result<T, RenameError>`.

use std::path::PathBuf;
use thiserror::Error;

/// Errors from rename operations.
#[derive(Debug, Error)]
pub enum RenameError {
    /// Package not found in workspace.
    #[error("Package '{0}' not found")]
    PackageNotFound(String),

    /// Target directory already exists.
    #[error("Target directory already exists: {0}")]
    DirectoryExists(PathBuf),

    /// Invalid package name.
    #[error("Invalid package name '{0}': {1}")]
    InvalidName(String, String),

    /// Invalid directory path.
    #[error("Invalid path '{0}': {1}")]
    InvalidPath(String, String),

    /// Workspace verification failed after rename.
    #[error("Workspace verification failed: {0}")]
    VerificationFailed(String),

    /// Rollback failed after commit error.
    #[error("Rollback failed: {0}")]
    RollbackFailed(String),

    /// Uncommitted changes in git workspace.
    #[error("Workspace has uncommitted changes")]
    DirtyWorkspace,

    /// User declined confirmation.
    ///
    /// Not a failure—used for control flow when user cancels.
    #[error("Operation cancelled by user")]
    Cancelled,

    /// File system operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// TOML parse or serialization error.
    #[error("TOML error: {0}")]
    Toml(#[from] toml_edit::TomlError),

    /// Cargo metadata command failed.
    #[error("Metadata error: {0}")]
    Metadata(#[from] cargo_metadata::Error),

    /// Regex compilation failed (indicates bug).
    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    /// Unexpected error.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Result type alias for cargo-rename operations.
pub type Result<T> = std::result::Result<T, RenameError>;
