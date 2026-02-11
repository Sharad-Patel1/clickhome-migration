//! Export extraction from TypeScript source using tree-sitter queries.
//!
//! This module provides functions to extract export information from parsed
//! TypeScript syntax trees for building the model registry.
//!
//! # Export Types Detected
//!
//! - `export class Foo { }` - Class exports
//! - `export interface Foo { }` - Interface exports
//! - `export { Foo, Bar }` - Named exports
//! - `export { Foo } from './foo'` - Re-exports
//!
//! # Examples
//!
//! ```ignore
//! use ch_ts_parser::exports::{extract_exports, get_export_query};
//!
//! let query = get_export_query()?;
//! let exports = extract_exports(&tree, source, query);
//!
//! for export in &exports {
//!     println!("{}: {:?}", export.name, export.kind);
//! }
//! ```

use std::sync::OnceLock;

use bumpalo::Bump;
use ch_core::{ExportKind, SourceLocation};
use smallvec::SmallVec;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Query, QueryCursor, Tree};

use crate::arena::{ArenaStr, StringInterner};

/// Tree-sitter query for extracting TypeScript exports.
///
/// This query captures:
/// - Export class declarations: `export class FooCodeGen { }`
/// - Export interface declarations: `export interface FooModel { }`
/// - Named export clauses: `export { Foo, Bar }`
/// - Re-exports: `export { Foo } from './foo'`
///
/// # Capture Names
///
/// - `export.class.name` - Class name in export class declaration
/// - `export.interface.name` - Interface name in export interface declaration
/// - `export.named.name` - Named export identifier
/// - `export.reexport.name` - Re-export identifier
/// - `export.reexport.source` - Re-export source path
pub const EXPORT_QUERY: &str = r"
; Export class declaration: export class FooCodeGen extends Bar { }
(export_statement
  declaration: (class_declaration
    name: (type_identifier) @export.class.name))

; Export interface declaration: export interface FooModel { }
(export_statement
  declaration: (interface_declaration
    name: (type_identifier) @export.interface.name))

; Named export clause: export { Foo, Bar }
(export_statement
  (export_clause
    (export_specifier
      name: (identifier) @export.named.name)))

; Re-export: export { Foo } from './foo'
(export_statement
  (export_clause
    (export_specifier
      name: (identifier) @export.reexport.name))
  source: (string) @export.reexport.source)
";

/// Capture index for `export.class.name`.
pub const CAPTURE_EXPORT_CLASS_NAME: u32 = 0;

/// Capture index for `export.interface.name`.
pub const CAPTURE_EXPORT_INTERFACE_NAME: u32 = 1;

/// Capture index for `export.named.name`.
pub const CAPTURE_EXPORT_NAMED_NAME: u32 = 2;

/// Capture index for `export.reexport.name`.
pub const CAPTURE_EXPORT_REEXPORT_NAME: u32 = 3;

/// Capture index for `export.reexport.source`.
pub const CAPTURE_EXPORT_REEXPORT_SOURCE: u32 = 4;

/// Global cache for the compiled export query (TypeScript).
static COMPILED_EXPORT_QUERY_TS: OnceLock<Query> = OnceLock::new();

/// Global cache for the compiled export query (TSX).
static COMPILED_EXPORT_QUERY_TSX: OnceLock<Query> = OnceLock::new();

/// Information about a single export from a TypeScript file.
///
/// # Examples
///
/// ```ignore
/// use ch_ts_parser::exports::ExportInfo;
/// use ch_core::{ExportKind, SourceLocation};
///
/// let export = ExportInfo {
///     name: "ActiveContractCodeGen".to_owned(),
///     kind: ExportKind::Class,
///     location: SourceLocation::new(10, 0, 245),
///     reexport_source: None,
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportInfo {
    /// The exported name (e.g., `ActiveContractCodeGen`).
    pub name: String,

    /// The kind of export (class, interface, named, re-export).
    pub kind: ExportKind,

    /// The location of the export in the source file.
    pub location: SourceLocation,

    /// For re-exports, the source path being re-exported from.
    pub reexport_source: Option<String>,
}

impl ExportInfo {
    /// Creates a new export info.
    #[must_use]
    pub fn new(name: impl Into<String>, kind: ExportKind, location: SourceLocation) -> Self {
        Self {
            name: name.into(),
            kind,
            location,
            reexport_source: None,
        }
    }

    /// Creates a new re-export info.
    #[must_use]
    pub fn reexport(
        name: impl Into<String>,
        source: impl Into<String>,
        location: SourceLocation,
    ) -> Self {
        Self {
            name: name.into(),
            kind: ExportKind::ReExport,
            location,
            reexport_source: Some(source.into()),
        }
    }

    /// Returns `true` if this is a class export.
    #[inline]
    #[must_use]
    pub const fn is_class(&self) -> bool {
        matches!(self.kind, ExportKind::Class)
    }

    /// Returns `true` if this is an interface export.
    #[inline]
    #[must_use]
    pub const fn is_interface(&self) -> bool {
        matches!(self.kind, ExportKind::Interface)
    }

    /// Returns `true` if this is a re-export.
    #[inline]
    #[must_use]
    pub const fn is_reexport(&self) -> bool {
        matches!(self.kind, ExportKind::ReExport)
    }
}

/// Arena-backed export information for efficient parsing.
///
/// This is the arena-backed equivalent of [`ExportInfo`]. All string data
/// is borrowed from a [`bumpalo::Bump`] arena.
#[derive(Debug, Clone)]
pub struct BumpExportInfo<'bump> {
    /// The exported name.
    pub name: ArenaStr<'bump>,

    /// The kind of export.
    pub kind: ExportKind,

    /// The location of the export in the source file.
    pub location: SourceLocation,

    /// For re-exports, the source path.
    pub reexport_source: Option<ArenaStr<'bump>>,
}

impl BumpExportInfo<'_> {
    /// Converts this arena-backed export info into an owned [`ExportInfo`].
    #[must_use]
    pub fn into_owned(self) -> ExportInfo {
        ExportInfo {
            name: self.name.as_str().to_owned(),
            kind: self.kind,
            location: self.location,
            reexport_source: self.reexport_source.map(|s| s.as_str().to_owned()),
        }
    }
}

impl From<BumpExportInfo<'_>> for ExportInfo {
    fn from(bump: BumpExportInfo<'_>) -> Self {
        bump.into_owned()
    }
}

/// Returns the compiled export query for TypeScript.
///
/// The query is compiled once and cached for all subsequent calls.
/// This function is thread-safe.
///
/// # Errors
///
/// Returns [`crate::ParseError`] if the query fails to compile.
pub fn get_typescript_export_query() -> Result<&'static Query, crate::ParseError> {
    if let Some(query) = COMPILED_EXPORT_QUERY_TS.get() {
        return Ok(query);
    }

    let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let query = compile_export_query(&language)?;

    Ok(COMPILED_EXPORT_QUERY_TS.get_or_init(|| query))
}

/// Returns the compiled export query for TSX.
///
/// The query is compiled once and cached for all subsequent calls.
/// This function is thread-safe.
///
/// # Errors
///
/// Returns [`crate::ParseError`] if the query fails to compile.
pub fn get_tsx_export_query() -> Result<&'static Query, crate::ParseError> {
    if let Some(query) = COMPILED_EXPORT_QUERY_TSX.get() {
        return Ok(query);
    }

    let language: Language = tree_sitter_typescript::LANGUAGE_TSX.into();
    let query = compile_export_query(&language)?;

    Ok(COMPILED_EXPORT_QUERY_TSX.get_or_init(|| query))
}

/// Compiles the export query for the given language.
fn compile_export_query(language: &Language) -> Result<Query, crate::ParseError> {
    Query::new(language, EXPORT_QUERY).map_err(|e| crate::ParseError::QueryCompile {
        offset: e.offset,
        kind: std::sync::Arc::new(e),
    })
}

/// Extracts all exports from a parsed TypeScript syntax tree.
///
/// This is a convenience wrapper around [`extract_exports_arena`] that manages
/// an internal arena and converts results to owned [`ExportInfo`].
///
/// # Arguments
///
/// * `tree` - The parsed syntax tree
/// * `source` - The original source code
/// * `query` - The pre-compiled export query
///
/// # Returns
///
/// A vector of [`ExportInfo`] for all detected exports.
pub fn extract_exports(tree: &Tree, source: &str, query: &Query) -> SmallVec<[ExportInfo; 16]> {
    let arena = Bump::new();
    let bump_exports = extract_exports_arena(&arena, tree, source, query);
    bump_exports
        .into_iter()
        .map(BumpExportInfo::into_owned)
        .collect()
}

/// Extracts all exports using arena allocation for zero-copy during extraction.
///
/// This is the high-performance version of export extraction.
///
/// # Arguments
///
/// * `arena` - The bump arena for string allocation
/// * `tree` - The parsed syntax tree
/// * `source` - The original source code
/// * `query` - The pre-compiled export query
///
/// # Returns
///
/// A vector of [`BumpExportInfo`] with string data borrowed from the arena.
pub fn extract_exports_arena<'bump>(
    arena: &'bump Bump,
    tree: &Tree,
    source: &str,
    query: &Query,
) -> SmallVec<[BumpExportInfo<'bump>; 16]> {
    let source_bytes = source.as_bytes();
    let root = tree.root_node();

    let mut interner = StringInterner::new(arena);
    let mut cursor = QueryCursor::new();
    let mut exports: SmallVec<[BumpExportInfo<'bump>; 16]> = SmallVec::new();

    // Track re-export sources - set when we encounter the source pattern in a re-export match
    let mut pending_reexport_source: Option<ArenaStr<'bump>> = None;

    cursor.set_max_start_depth(None);
    let mut matches = cursor.matches(query, root, source_bytes);

    while let Some(match_) = matches.next() {
        for capture in match_.captures {
            let node = capture.node;
            let capture_index = capture.index;

            match capture_index {
                idx if idx == CAPTURE_EXPORT_CLASS_NAME => {
                    if let Some(name) = node_text(node, source_bytes) {
                        let interned = interner.intern(name);
                        let location = node_to_location(node);
                        exports.push(BumpExportInfo {
                            name: interned,
                            kind: ExportKind::Class,
                            location,
                            reexport_source: None,
                        });
                    }
                }
                idx if idx == CAPTURE_EXPORT_INTERFACE_NAME => {
                    if let Some(name) = node_text(node, source_bytes) {
                        let interned = interner.intern(name);
                        let location = node_to_location(node);
                        exports.push(BumpExportInfo {
                            name: interned,
                            kind: ExportKind::Interface,
                            location,
                            reexport_source: None,
                        });
                    }
                }
                idx if idx == CAPTURE_EXPORT_NAMED_NAME => {
                    if let Some(name) = node_text(node, source_bytes) {
                        let interned = interner.intern(name);
                        let location = node_to_location(node);
                        exports.push(BumpExportInfo {
                            name: interned,
                            kind: ExportKind::Named,
                            location,
                            reexport_source: None,
                        });
                    }
                }
                idx if idx == CAPTURE_EXPORT_REEXPORT_SOURCE => {
                    if let Some(source_path) = node_text(node, source_bytes) {
                        pending_reexport_source = Some(interner.intern(source_path));
                    }
                }
                idx if idx == CAPTURE_EXPORT_REEXPORT_NAME => {
                    if let Some(name) = node_text(node, source_bytes) {
                        let interned = interner.intern(name);
                        let location = node_to_location(node);
                        exports.push(BumpExportInfo {
                            name: interned,
                            kind: ExportKind::ReExport,
                            location,
                            reexport_source: pending_reexport_source,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // Sort by location for consistent ordering
    exports.sort_by_key(|e| (e.location.line, e.location.column));

    exports
}

/// Extracts text from a node.
fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let start = node.start_byte();
    let end = node.end_byte();
    std::str::from_utf8(source.get(start..end)?).ok()
}

/// Converts a node's position to a [`SourceLocation`].
#[allow(clippy::cast_possible_truncation)]
fn node_to_location(node: Node<'_>) -> SourceLocation {
    let start = node.start_position();
    SourceLocation::new(
        start.row as u32 + 1, // Convert 0-indexed to 1-indexed
        start.column as u32,
        node.start_byte() as u32,
    )
}

/// Converts a kebab-case filename to `PascalCase`.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(kebab_to_pascal("active-contract"), "ActiveContract");
/// assert_eq!(kebab_to_pascal("my-model-name"), "MyModelName");
/// ```
#[must_use]
pub fn kebab_to_pascal(kebab: &str) -> String {
    // Remove .ts or .tsx extension if present
    let name = kebab
        .strip_suffix(".ts")
        .or_else(|| kebab.strip_suffix(".tsx"))
        .unwrap_or(kebab);

    let mut result = String::with_capacity(name.len());
    let mut capitalize_next = true;

    for c in name.chars() {
        if c == '-' || c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Converts a `PascalCase` or `camelCase` name to kebab-case.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(pascal_to_kebab("ActiveContract"), "active-contract");
/// assert_eq!(pascal_to_kebab("MyModelName"), "my-model-name");
/// ```
#[must_use]
pub fn pascal_to_kebab(pascal: &str) -> String {
    let mut result = String::with_capacity(pascal.len() + 5);

    for (i, c) in pascal.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn create_parser() -> Parser {
        let mut parser = Parser::new();
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        parser
            .set_language(&language)
            .expect("Failed to set language");
        parser
    }

    fn create_query() -> Query {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        Query::new(&language, EXPORT_QUERY).expect("Query should compile")
    }

    #[test]
    fn test_export_query_compiles() {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let result = compile_export_query(&language);
        assert!(result.is_ok(), "Query should compile: {result:?}");
    }

    #[test]
    fn test_extract_class_export() {
        let source = r#"export class ActiveContractCodeGen extends BaseCodeGen { }"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let exports = extract_exports(&tree, source, &query);
        assert_eq!(exports.len(), 1);

        let export = &exports[0];
        assert_eq!(export.name, "ActiveContractCodeGen");
        assert_eq!(export.kind, ExportKind::Class);
        assert!(export.reexport_source.is_none());
    }

    #[test]
    fn test_extract_interface_export() {
        let source = r#"export interface ActiveContractModel { id: string; }"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let exports = extract_exports(&tree, source, &query);
        assert_eq!(exports.len(), 1);

        let export = &exports[0];
        assert_eq!(export.name, "ActiveContractModel");
        assert_eq!(export.kind, ExportKind::Interface);
    }

    #[test]
    fn test_extract_named_export() {
        let source = r#"export { Foo, Bar, Baz };"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let exports = extract_exports(&tree, source, &query);
        assert_eq!(exports.len(), 3);

        let names: Vec<_> = exports.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(names.contains(&"Bar"));
        assert!(names.contains(&"Baz"));

        assert!(exports.iter().all(|e| e.kind == ExportKind::Named));
    }

    #[test]
    fn test_extract_reexport() {
        let source = r#"export { Foo, Bar } from './foo';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let exports = extract_exports(&tree, source, &query);
        assert_eq!(exports.len(), 2);

        // Re-exports should have the source path
        assert!(exports.iter().all(|e| e.kind == ExportKind::ReExport));
        assert!(exports
            .iter()
            .all(|e| e.reexport_source.as_deref() == Some("'./foo'")));
    }

    #[test]
    fn test_extract_multiple_export_types() {
        let source = r#"
export class FooCodeGen { }
export interface FooModel { }
export { Bar };
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let exports = extract_exports(&tree, source, &query);
        assert_eq!(exports.len(), 3);

        let class_export = exports.iter().find(|e| e.name == "FooCodeGen");
        let interface_export = exports.iter().find(|e| e.name == "FooModel");
        let named_export = exports.iter().find(|e| e.name == "Bar");

        assert!(class_export.is_some());
        assert!(interface_export.is_some());
        assert!(named_export.is_some());

        assert_eq!(class_export.unwrap().kind, ExportKind::Class);
        assert_eq!(interface_export.unwrap().kind, ExportKind::Interface);
        assert_eq!(named_export.unwrap().kind, ExportKind::Named);
    }

    #[test]
    fn test_extract_no_exports() {
        let source = r#"
const x = 1;
function foo() { return x; }
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let exports = extract_exports(&tree, source, &query);
        assert!(exports.is_empty());
    }

    #[test]
    fn test_export_info_methods() {
        let class_export = ExportInfo::new("Foo", ExportKind::Class, SourceLocation::default());
        assert!(class_export.is_class());
        assert!(!class_export.is_interface());
        assert!(!class_export.is_reexport());

        let interface_export =
            ExportInfo::new("Bar", ExportKind::Interface, SourceLocation::default());
        assert!(!interface_export.is_class());
        assert!(interface_export.is_interface());
        assert!(!interface_export.is_reexport());

        let reexport = ExportInfo::reexport("Baz", "./baz", SourceLocation::default());
        assert!(!reexport.is_class());
        assert!(!reexport.is_interface());
        assert!(reexport.is_reexport());
        assert_eq!(reexport.reexport_source.as_deref(), Some("./baz"));
    }

    #[test]
    fn test_kebab_to_pascal() {
        assert_eq!(kebab_to_pascal("active-contract"), "ActiveContract");
        assert_eq!(kebab_to_pascal("my-model-name"), "MyModelName");
        assert_eq!(kebab_to_pascal("simple"), "Simple");
        assert_eq!(kebab_to_pascal("a-b-c"), "ABC");
        assert_eq!(kebab_to_pascal("active-contract.ts"), "ActiveContract");
        assert_eq!(kebab_to_pascal("active-contract.tsx"), "ActiveContract");
        assert_eq!(kebab_to_pascal("underscore_case"), "UnderscoreCase");
    }

    #[test]
    fn test_pascal_to_kebab() {
        assert_eq!(pascal_to_kebab("ActiveContract"), "active-contract");
        assert_eq!(pascal_to_kebab("MyModelName"), "my-model-name");
        assert_eq!(pascal_to_kebab("Simple"), "simple");
        assert_eq!(pascal_to_kebab("ABC"), "a-b-c");
    }

    #[test]
    fn test_arena_extract_exports() {
        let arena = Bump::new();
        let source = r#"export class FooCodeGen { }"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let bump_exports = extract_exports_arena(&arena, &tree, source, &query);
        assert_eq!(bump_exports.len(), 1);

        let export = &bump_exports[0];
        assert_eq!(export.name.as_str(), "FooCodeGen");
        assert_eq!(export.kind, ExportKind::Class);

        // Convert to owned
        let owned: ExportInfo = bump_exports.into_iter().next().unwrap().into();
        assert_eq!(owned.name, "FooCodeGen");
    }
}
