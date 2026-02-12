//! Pre-compiled tree-sitter queries for TypeScript import and relation extraction.
//!
//! This module provides the [`IMPORT_QUERY`] constant containing S-expression
//! patterns for matching import statements, as well as relation query patterns
//! used to extract CST/AST evidence. Queries are lazily compiled and cached.

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

/// Tree-sitter query for extracting model relation evidence.
///
/// This query captures relation targets and call/member contexts for:
///
/// - `class ... extends ...`
/// - `class ... implements ...`
/// - `interface ... extends ...`
/// - Type annotations on properties/fields/params/returns
/// - Constructor calls (`new Foo(...)`)
/// - Factory/member calls (`Foo.toApi()`, `Foo.toFormGroup()`, `modelMap.*`)
pub const RELATION_QUERY: &str = r"
; class declaration extends clause
(class_declaration
  (class_heritage
    (extends_clause
      value: (_) @relation.extends.target)))

; class declaration implements clause
(class_declaration
  (class_heritage
    (implements_clause
      (type) @relation.implements.target)))

; interface declaration extends clause
(interface_declaration
  (extends_type_clause
    type: (_) @relation.interface_extends.target))

; interface property type references
(property_signature
  type: (type_annotation
    (_) @relation.type_ref.property))

; class field type references
(public_field_definition
  type: (type_annotation
    (_) @relation.type_ref.field))

; parameter type references
(required_parameter
  type: (type_annotation
    (_) @relation.service.param))

(optional_parameter
  type: (type_annotation
    (_) @relation.service.param))

; return type references
(function_declaration
  return_type: (type_annotation
    (_) @relation.service.return))

(method_definition
  return_type: (type_annotation
    (_) @relation.service.return))

(method_signature
  return_type: (type_annotation
    (_) @relation.service.return))

(arrow_function
  return_type: (type_annotation
    (_) @relation.service.return))

; constructor relations
(new_expression
  constructor: (_) @relation.constructs.target)

; member call relations (factory/modelMap-style)
(call_expression
  function: (member_expression
    object: (_) @relation.call.object
    property: (property_identifier) @relation.call.property) @relation.call.member)
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

/// Capture index for `relation.extends.target`.
pub const CAPTURE_RELATION_EXTENDS_TARGET: u32 = 0;

/// Capture index for `relation.implements.target`.
pub const CAPTURE_RELATION_IMPLEMENTS_TARGET: u32 = 1;

/// Capture index for `relation.interface_extends.target`.
pub const CAPTURE_RELATION_INTERFACE_EXTENDS_TARGET: u32 = 2;

/// Capture index for `relation.type_ref.property`.
pub const CAPTURE_RELATION_TYPE_REF_PROPERTY: u32 = 3;

/// Capture index for `relation.type_ref.field`.
pub const CAPTURE_RELATION_TYPE_REF_FIELD: u32 = 4;

/// Capture index for `relation.service.param`.
pub const CAPTURE_RELATION_SERVICE_PARAM: u32 = 5;

/// Capture index for `relation.service.return`.
pub const CAPTURE_RELATION_SERVICE_RETURN: u32 = 6;

/// Capture index for `relation.constructs.target`.
pub const CAPTURE_RELATION_CONSTRUCTS_TARGET: u32 = 7;

/// Capture index for `relation.call.object`.
pub const CAPTURE_RELATION_CALL_OBJECT: u32 = 8;

/// Capture index for `relation.call.property`.
pub const CAPTURE_RELATION_CALL_PROPERTY: u32 = 9;

/// Capture index for `relation.call.member`.
pub const CAPTURE_RELATION_CALL_MEMBER: u32 = 10;

/// Global cache for the compiled import query (TypeScript).
static COMPILED_QUERY_TS: OnceLock<Query> = OnceLock::new();

/// Global cache for the compiled import query (TSX).
static COMPILED_QUERY_TSX: OnceLock<Query> = OnceLock::new();

/// Global cache for the compiled relation query (TypeScript).
static COMPILED_RELATION_QUERY_TS: OnceLock<Query> = OnceLock::new();

/// Global cache for the compiled relation query (TSX).
static COMPILED_RELATION_QUERY_TSX: OnceLock<Query> = OnceLock::new();

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

/// Returns the compiled relation query for TypeScript.
///
/// The query is compiled once and cached for all subsequent calls.
/// This function is thread-safe.
///
/// # Errors
///
/// Returns [`ParseError::QueryCompile`] if the query fails to compile.
pub fn get_typescript_relation_query() -> Result<&'static Query, ParseError> {
    if let Some(query) = COMPILED_RELATION_QUERY_TS.get() {
        return Ok(query);
    }

    let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let query = compile_relation_query(&language)?;

    Ok(COMPILED_RELATION_QUERY_TS.get_or_init(|| query))
}

/// Returns the compiled relation query for TSX.
///
/// The query is compiled once and cached for all subsequent calls.
/// This function is thread-safe.
///
/// # Errors
///
/// Returns [`ParseError::QueryCompile`] if the query fails to compile.
pub fn get_tsx_relation_query() -> Result<&'static Query, ParseError> {
    if let Some(query) = COMPILED_RELATION_QUERY_TSX.get() {
        return Ok(query);
    }

    let language: Language = tree_sitter_typescript::LANGUAGE_TSX.into();
    let query = compile_relation_query(&language)?;

    Ok(COMPILED_RELATION_QUERY_TSX.get_or_init(|| query))
}

/// Compiles the import query for the given language.
fn compile_query(language: &Language) -> Result<Query, ParseError> {
    Query::new(language, IMPORT_QUERY)
        .map_err(|e| ParseError::QueryCompile { offset: e.offset, kind: std::sync::Arc::new(e) })
}

/// Compiles the relation query for the given language.
fn compile_relation_query(language: &Language) -> Result<Query, ParseError> {
    Query::new(language, RELATION_QUERY)
        .map_err(|e| ParseError::QueryCompile { offset: e.offset, kind: std::sync::Arc::new(e) })
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

    #[test]
    fn test_relation_query_compiles() {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let result = compile_relation_query(&language);
        assert!(result.is_ok(), "Relation query should compile: {result:?}");
    }

    #[test]
    fn test_relation_capture_names() {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let query = compile_relation_query(&language).expect("Relation query should compile");

        let names = query.capture_names();
        assert!(names.contains(&"relation.extends.target"));
        assert!(names.contains(&"relation.implements.target"));
        assert!(names.contains(&"relation.interface_extends.target"));
        assert!(names.contains(&"relation.type_ref.property"));
        assert!(names.contains(&"relation.type_ref.field"));
        assert!(names.contains(&"relation.service.param"));
        assert!(names.contains(&"relation.service.return"));
        assert!(names.contains(&"relation.constructs.target"));
        assert!(names.contains(&"relation.call.object"));
        assert!(names.contains(&"relation.call.property"));
        assert!(names.contains(&"relation.call.member"));
    }
}
