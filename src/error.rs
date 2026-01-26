use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenameError {
    #[error("Package '{0}' not found")]
    PackageNotFound(String),

    #[error("Target directory already exists: {0}")]
    DirectoryExists(PathBuf),

    #[error("Invalid package name '{0}': {1}")]
    InvalidName(String, String),

    #[error("Workspace verification failed: {0}")]
    VerificationFailed(String),

    #[error("Rollback failed: {0}")]
    RollbackFailed(String),

    #[error("Workspace has uncommitted changes")]
    DirtyWorkspace,

    #[error("Operation cancelled by user")]
    Cancelled,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("TOML error: {0}")]
    Toml(#[from] toml_edit::TomlError),

    #[error("Metadata error: {0}")]
    Metadata(#[from] cargo_metadata::Error),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, RenameError>;
