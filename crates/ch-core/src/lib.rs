//! Core types, errors, and utilities for the ch-migration tool.
//!
//! This crate provides the foundational types used across the workspace for
//! building a TUI application that helps developers migrate TypeScript models
//! from the legacy `shared/` directory to `shared_2023/` in the `ClickHome`
//! enterprise web application.
//!
//! # Overview
//!
//! The ch-core crate is the foundation layer with no async dependencies. It provides:
//!
//! - **Error types**: [`ConfigError`] for configuration-related errors
//! - **Configuration**: [`Config`], [`ScanConfig`], [`WatchConfig`], [`TuiConfig`]
//! - **Domain types**: [`FileInfo`], [`ImportInfo`], [`ModelReference`], [`MigrationStatus`]
//! - **Hash utilities**: [`FxHashMap`], [`FxHashSet`] (faster than std for string keys)
//!
//! # Crate Dependencies
//!
//! This crate is designed to be the base dependency for all other crates in the
//! workspace. It has minimal dependencies and no async runtime requirements.
//!
//! ```text
//! ch-cli ──► ch-tui ──► ch-scanner ──► ch-ts-parser ──► ch-core
//!                   ├─► ch-watcher ─────────────────────────►
//! ```
//!
//! # Examples
//!
//! ## Working with Configuration
//!
//! ```
//! use ch_core::{Config, ScanConfig};
//! use camino::Utf8PathBuf;
//!
//! // Create a configuration with custom scan settings
//! let mut config = Config::default();
//! config.scan.root_path = Utf8PathBuf::from("/path/to/WebApp.Desktop/src");
//!
//! // Serialize to JSON
//! let json = serde_json::to_string_pretty(&config).unwrap();
//! ```
//!
//! ## Tracking Migration Status
//!
//! ```
//! use ch_core::{FileInfo, FileId, MigrationStatus};
//! use camino::Utf8PathBuf;
//!
//! let mut file = FileInfo::new(
//!     FileId::new(1),
//!     Utf8PathBuf::from("src/components/foo.component.ts"),
//! );
//!
//! file.status = MigrationStatus::Legacy;
//! assert!(file.needs_migration());
//!
//! file.status = MigrationStatus::Migrated;
//! assert!(!file.needs_migration());
//! ```
//!
//! ## Using Fast Hash Maps
//!
//! ```
//! use ch_core::{FxHashMap, fx_hash_map};
//!
//! // Create a hash map optimized for string keys
//! let mut cache: FxHashMap<String, i32> = fx_hash_map();
//! cache.insert("key".to_owned(), 42);
//! ```

#![deny(clippy::all)]
#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod hash;
pub mod types;

// Re-export configuration types
pub use config::{ColorScheme, Config, ScanConfig, TuiConfig, WatchConfig};

// Re-export error types
pub use error::ConfigError;

// Re-export hash utilities
pub use hash::{
    fx_hash_map, fx_hash_map_with_capacity, fx_hash_set, fx_hash_set_with_capacity, FxBuildHasher,
    FxHashMap, FxHashSet,
};

// Re-export domain types
pub use types::{
    FileId, FileInfo, ImportInfo, ImportKind, MigrationStatus, ModelCategory, ModelReference,
    ModelSource, SourceLocation,
};
