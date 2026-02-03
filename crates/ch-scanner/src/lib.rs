//! Filesystem scanner for TypeScript files with parallel analysis.
//!
//! This crate handles:
//!
//! - Directory traversal respecting `.gitignore` patterns
//! - Parallel file processing with rayon
//! - TypeScript file filtering (`.ts`, `.tsx`)
//! - Analysis result caching with `dashmap`
//! - Statistics aggregation for migration progress

#![deny(clippy::all)]
#![warn(missing_docs)]

// TODO: Add modules during implementation
// pub mod walker;
// pub mod analyzer;
// pub mod cache;
// pub mod stats;
