//! Core types, errors, and utilities for the ch-migration tool.
//!
//! This crate provides the foundational types used across the workspace:
//!
//! - Error types for consistent error handling
//! - Configuration structures
//! - Domain types (`FileInfo`, `ModelReference`, `MigrationStatus`)
//! - Type aliases for `FxHashMap`/`FxHashSet` (faster than std)
//! - Common traits

#![deny(clippy::all)]
#![warn(missing_docs)]

// TODO: Add modules during implementation
// pub mod config;
// pub mod error;
// pub mod types;
// pub mod hash;
