//! Cargo manifest manipulation.
//!
//! This module provides functions for updating `Cargo.toml` files during
//! package rename operations. It is split into three concerns:
//!
//! - **`package`**: Updates to the renamed package's own manifest
//! - **`workspace`**: Updates to workspace-level configuration
//! - **`dependency`**: Updates to dependency references in other packages
//!
//! All functions use `toml_edit` or regex-based transformations to preserve
//! formatting, comments, and structure.

pub mod dependency;
pub mod package;
pub mod workspace;

pub use dependency::update_dependent_manifest;
pub use package::update_package_name;
pub use workspace::update_workspace_manifest;
