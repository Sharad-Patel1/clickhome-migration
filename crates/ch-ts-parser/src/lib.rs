//! TypeScript parser using tree-sitter for import and model detection.
//!
//! This crate provides incremental parsing of TypeScript files to:
//!
//! - Extract import statements (static and dynamic)
//! - Detect model/interface references from shared directories
//! - Support incremental re-parsing on file changes
//! - Efficiently categorize imports as legacy (`shared/`) or new (`shared_2023/`)
//!
//! # Overview
//!
//! The main entry point is [`TsParser`], which wraps a tree-sitter parser
//! configured for TypeScript. Use it to parse source files and extract
//! import information:
//!
//! ```
//! use ch_ts_parser::TsParser;
//!
//! let mut parser = TsParser::new()?;
//! let source = r#"
//!     import { ActiveContract } from '../shared/models/active-contract';
//!     import { NewModel } from '../shared_2023/models/new-model';
//! "#;
//!
//! let result = parser.parse(source)?;
//!
//! for import in &result.imports {
//!     if import.is_legacy_import() {
//!         println!("Needs migration: {}", import.path);
//!     }
//! }
//! # Ok::<(), ch_ts_parser::ParseError>(())
//! ```
//!
//! # Import Detection
//!
//! The parser detects all TypeScript import patterns:
//!
//! | Pattern | Example | Kind |
//! |---------|---------|------|
//! | Named | `import { Foo, Bar } from './path'` | `Named` |
//! | Default | `import Foo from './path'` | `Default` |
//! | Namespace | `import * as Foo from './path'` | `Namespace` |
//! | Side-effect | `import './path'` | `SideEffect` |
//! | Type-only | `import type { Foo } from './path'` | `TypeOnly` |
//! | Dynamic | `await import('./path')` | `Dynamic` |
//!
//! # Model Source Detection
//!
//! For imports from `ClickHome`'s model directories, the parser determines
//! whether they reference the legacy `shared/` or new `shared_2023/` directory:
//!
//! ```
//! use ch_ts_parser::detect_model_source;
//! use ch_core::ModelSource;
//!
//! assert_eq!(
//!     detect_model_source("'../shared/models/foo'"),
//!     Some(ModelSource::SharedLegacy)
//! );
//!
//! assert_eq!(
//!     detect_model_source("'../shared_2023/models/foo'"),
//!     Some(ModelSource::Shared2023)
//! );
//!
//! assert_eq!(detect_model_source("'@angular/core'"), None);
//! ```
//!
//! # Incremental Parsing
//!
//! For efficient re-parsing when files change (e.g., from file watching),
//! use [`TsParser::parse_incremental`]:
//!
//! ```ignore
//! use tree_sitter::{InputEdit, Point};
//!
//! // After editing a file, provide the edit information
//! let edit = InputEdit {
//!     start_byte: 10,
//!     old_end_byte: 15,
//!     new_end_byte: 20,
//!     start_position: Point::new(0, 10),
//!     old_end_position: Point::new(0, 15),
//!     new_end_position: Point::new(0, 20),
//! };
//!
//! let new_result = parser.parse_incremental(new_source, &old_result.tree, &edit)?;
//! ```
//!
//! # Arena-Based Parsing
//!
//! For high-performance parallel scanning, use [`ArenaParser`] with
//! [`bumpalo::Bump`] arenas. This eliminates per-string heap allocations
//! during import extraction:
//!
//! ```
//! use bumpalo::Bump;
//! use ch_ts_parser::ArenaParser;
//!
//! let mut parser = ArenaParser::new()?;
//! let arena = Bump::new();
//! let source = r#"import { Foo } from '../shared/models/foo';"#;
//!
//! // Parse with arena allocation
//! let result = parser.parse_with_arena(&arena, source)?;
//!
//! // Process arena-backed data
//! for import in &result.imports {
//!     if import.is_legacy_import() {
//!         println!("Legacy: {}", import.path);
//!     }
//! }
//!
//! // Convert to owned when arena lifetime ends
//! let owned = result.into_owned();
//! # Ok::<(), ch_ts_parser::ParseError>(())
//! ```
//!
//! For parallel scanning with rayon, use [`bumpalo_herd::Herd`]:
//!
//! ```ignore
//! use bumpalo_herd::Herd;
//! use ch_ts_parser::ArenaParser;
//! use rayon::prelude::*;
//!
//! let herd = Herd::new();
//! let results: Vec<_> = files
//!     .par_iter()
//!     .map_init(
//!         || (ArenaParser::new().unwrap(), herd.get()),
//!         |(parser, member), source| {
//!             let result = parser.parse_with_arena(member.as_bump(), source);
//!             result.map(|r| r.into_owned())
//!         },
//!     )
//!     .collect();
//! ```
//!
//! # Thread Safety
//!
//! Both [`TsParser`] and [`ArenaParser`] are `Send` but not `Sync`.
//! For parallel scanning with rayon:
//!
//! - Create one parser per thread using `thread_local!`
//! - Or create a new parser for each parallel task
//! - Or use [`ArenaParser`] with [`bumpalo_herd::Herd`]
//!
//! The underlying tree-sitter queries are thread-safe and shared globally.
//!
//! # Performance
//!
//! The crate is optimized for performance:
//!
//! - Pre-compiled tree-sitter queries (compiled once, reused)
//! - Incremental parsing for file changes (O(edit size) not O(file size))
//! - Arena allocation for zero per-string heap allocations
//! - String interning for path deduplication
//! - `SmallVec` for import lists (avoids heap allocation for typical files)
//! - Zero-copy path detection (operates on string slices)

#![deny(clippy::all)]
#![warn(missing_docs)]

pub mod arena;
pub mod error;
mod import;
mod parser;
pub mod queries;
pub mod source;

// Re-export main types for convenient access
pub use error::ParseError;
pub use parser::{ArenaParser, BumpParseResult, ParseResult, TsParser};
pub use source::{detect_model_source, detect_model_source_with, ModelPathMatcher};

// Re-export arena types for ch-scanner integration
pub use arena::{ArenaStr, BumpImportBuilder, BumpImportInfo, StringInterner};

// Re-export import extraction functions
pub use import::{extract_imports, extract_imports_arena};

// Re-export tree-sitter types that appear in our public API
pub use tree_sitter::InputEdit;

// Re-export bumpalo for convenience (consumers need it for ArenaParser)
pub use bumpalo::Bump;
