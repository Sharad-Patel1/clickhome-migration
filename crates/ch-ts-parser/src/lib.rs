//! TypeScript parser using tree-sitter for import and model detection.
//!
//! This crate provides incremental parsing of TypeScript files to:
//!
//! - Extract import statements (static and dynamic)
//! - Detect model/interface references from shared directories
//! - Support incremental re-parsing on file changes
//! - Efficient memory usage via arena allocation (bumpalo)

#![deny(clippy::all)]
#![warn(missing_docs)]

// TODO: Add modules during implementation
// pub mod parser;
// pub mod import;
// pub mod model_ref;
// pub mod query;
// pub mod arena;
