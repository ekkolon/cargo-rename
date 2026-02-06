//! Cargo manifest manipulation.
//!
//! Updates `Cargo.toml` files during package rename operations:
//! - **`package`**: Renamed package's own manifest
//! - **`workspace`**: Workspace-level configuration
//! - **`dependency`**: Dependency references in other packages

pub mod dependency;
pub mod package;
pub mod workspace;

pub use dependency::update_dependent_manifest;
pub use package::update_package_name;
pub use workspace::update_workspace_manifest;
