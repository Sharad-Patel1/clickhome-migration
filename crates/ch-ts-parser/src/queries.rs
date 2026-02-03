//! Pre-compiled tree-sitter queries for TypeScript import extraction.
//!
//! This module provides the [`IMPORT_QUERY`] constant containing S-expression
//! patterns for matching import statements, and [`get_import_query`] for
//! lazily compiling and caching the query.

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use crate::error::ParseError;

/// Tree-sitter query for extracting TypeScript imports.
///
/// This query captures:
/// - Static import statements with their source paths
/// - Named imports (individual identifiers)
/// - Default imports
/// - Namespace imports (`import * as`)
/// - Dynamic imports (`import()` expressions)
///
/// # Capture Names
///
/// - `import.source` - The import path string literal
/// - `import.statement` - The full `import_statement` node
/// - `import.named.name` - Named import identifiers
/// - `import.default.name` - Default import identifier
/// - `import.namespace.name` - Namespace import identifier
/// - `import.dynamic.source` - Dynamic import path string
pub const IMPORT_QUERY: &str = r"
; Static imports with source path
(import_statement
  source: (string) @import.source) @import.statement

; Named imports: import { Foo, Bar } from '...'
(import_statement
  (import_clause
    (named_imports
      (import_specifier
        name: (identifier) @import.named.name))))

; Default imports: import Foo from '...'
(import_statement
  (import_clause
    (identifier) @import.default.name))

; Namespace imports: import * as Foo from '...'
(import_statement
  (import_clause
    (namespace_import
      (identifier) @import.namespace.name)))

; Dynamic imports: import('./path') or await import('./path')
(call_expression
  function: (import)
  arguments: (arguments
    (string) @import.dynamic.source))
";

/// Capture index for `import.source`.
pub const CAPTURE_IMPORT_SOURCE: u32 = 0;

/// Capture index for `import.statement`.
pub const CAPTURE_IMPORT_STATEMENT: u32 = 1;

/// Capture index for `import.named.name`.
pub const CAPTURE_IMPORT_NAMED_NAME: u32 = 2;

/// Capture index for `import.default.name`.
pub const CAPTURE_IMPORT_DEFAULT_NAME: u32 = 3;

/// Capture index for `import.namespace.name`.
pub const CAPTURE_IMPORT_NAMESPACE_NAME: u32 = 4;

/// Capture index for `import.dynamic.source`.
pub const CAPTURE_IMPORT_DYNAMIC_SOURCE: u32 = 5;

/// Global cache for the compiled import query (TypeScript).
static COMPILED_QUERY_TS: OnceLock<Query> = OnceLock::new();

/// Global cache for the compiled import query (TSX).
static COMPILED_QUERY_TSX: OnceLock<Query> = OnceLock::new();

/// Returns the compiled import query for TypeScript.
///
/// The query is compiled once and cached for all subsequent calls.
/// This function is thread-safe.
///
/// # Errors
///
/// Returns [`ParseError::QueryCompile`] if the query fails to compile.
///
/// # Examples
///
/// ```ignore
/// use ch_ts_parser::queries::get_typescript_import_query;
///
/// let query = get_typescript_import_query()?;
/// ```
pub fn get_typescript_import_query() -> Result<&'static Query, ParseError> {
    if let Some(query) = COMPILED_QUERY_TS.get() {
        return Ok(query);
    }

    let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let query = compile_query(&language)?;

    Ok(COMPILED_QUERY_TS.get_or_init(|| query))
}

/// Returns the compiled import query for TSX.
///
/// The query is compiled once and cached for all subsequent calls.
/// This function is thread-safe.
///
/// # Errors
///
/// Returns [`ParseError::QueryCompile`] if the query fails to compile.
///
/// # Examples
///
/// ```ignore
/// use ch_ts_parser::queries::get_tsx_import_query;
///
/// let query = get_tsx_import_query()?;
/// ```
pub fn get_tsx_import_query() -> Result<&'static Query, ParseError> {
    if let Some(query) = COMPILED_QUERY_TSX.get() {
        return Ok(query);
    }

    let language: Language = tree_sitter_typescript::LANGUAGE_TSX.into();
    let query = compile_query(&language)?;

    Ok(COMPILED_QUERY_TSX.get_or_init(|| query))
}

/// Compiles the import query for the given language.
fn compile_query(language: &Language) -> Result<Query, ParseError> {
    Query::new(language, IMPORT_QUERY).map_err(|e| ParseError::QueryCompile {
        offset: e.offset,
        kind: e,
    })
}

/// Returns the capture name for a given capture index.
///
/// # Arguments
///
/// * `query` - The compiled query
/// * `index` - The capture index
///
/// # Returns
///
/// The capture name as a string slice, or `None` if the index is invalid.
#[inline]
pub fn capture_name(query: &Query, index: u32) -> Option<&str> {
    query.capture_names().get(index as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_compiles() {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let result = compile_query(&language);
        assert!(result.is_ok(), "Query should compile: {result:?}");
    }

    #[test]
    fn test_capture_names() {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let query = compile_query(&language).expect("Query should compile");

        let names = query.capture_names();
        assert!(names.contains(&"import.source"));
        assert!(names.contains(&"import.statement"));
        assert!(names.contains(&"import.named.name"));
        assert!(names.contains(&"import.default.name"));
        assert!(names.contains(&"import.namespace.name"));
        assert!(names.contains(&"import.dynamic.source"));
    }

    #[test]
    fn test_query_pattern_count() {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let query = compile_query(&language).expect("Query should compile");

        // We have 5 patterns in our query
        assert_eq!(query.pattern_count(), 5);
    }
}
