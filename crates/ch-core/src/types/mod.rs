//! Domain types for the ch-migration tool.
//!
//! This module contains all the core domain types used throughout the application
//! for representing files, imports, models, and migration status.
//!
//! # Module Organization
//!
//! - [`file`] - File information and identifiers
//! - [`import`] - Import statements and their metadata
//! - [`location`] - Source code locations
//! - [`model`] - Model references and categories
//! - [`status`] - Migration status tracking
//!
//! # Re-exports
//!
//! All public types are re-exported at this module level for convenience:
//!
//! ```
//! use ch_core::types::{FileId, FileInfo, ImportInfo, MigrationStatus};
//! ```
//!
//! They are also re-exported at the crate root:
//!
//! ```
//! use ch_core::{FileId, FileInfo, ImportInfo, MigrationStatus};
//! ```

mod file;
mod import;
mod location;
mod model;
mod status;

// Re-export all public types
pub use file::{FileId, FileInfo};
pub use import::{ImportInfo, ImportKind};
pub use location::SourceLocation;
pub use model::{
    ExportKind, ModelCategory, ModelDefinition, ModelReference, ModelRegistry, ModelSource,
};
pub use status::MigrationStatus;
