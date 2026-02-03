//! Import extraction from TypeScript source using tree-sitter queries.
//!
//! This module provides functions to extract [`ImportInfo`] from parsed
//! TypeScript syntax trees.

use ch_core::{ImportInfo, ImportKind, SourceLocation};
use rustc_hash::FxHashMap;
use smallvec::{smallvec, SmallVec};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, Tree};

use crate::queries::{
    CAPTURE_IMPORT_DEFAULT_NAME, CAPTURE_IMPORT_DYNAMIC_SOURCE, CAPTURE_IMPORT_NAMED_NAME,
    CAPTURE_IMPORT_NAMESPACE_NAME, CAPTURE_IMPORT_SOURCE, CAPTURE_IMPORT_STATEMENT,
};
use crate::source::detect_model_source;

/// Extracts all imports from a parsed TypeScript syntax tree.
///
/// Uses pre-compiled tree-sitter queries to efficiently find and extract
/// import statements from the source code.
///
/// # Arguments
///
/// * `tree` - The parsed syntax tree
/// * `source` - The original source code (needed to extract text from nodes)
/// * `query` - The pre-compiled import query
///
/// # Returns
///
/// A vector of [`ImportInfo`] for all detected imports, including:
/// - Static imports (named, default, namespace, side-effect, type-only)
/// - Dynamic imports (`import()` expressions)
///
/// # Examples
///
/// ```ignore
/// let tree = parser.parse(source, None)?;
/// let query = get_import_query(&language)?;
/// let imports = extract_imports(&tree, source, query);
/// ```
pub fn extract_imports(tree: &Tree, source: &str, query: &Query) -> SmallVec<[ImportInfo; 8]> {
    let source_bytes = source.as_bytes();
    let root = tree.root_node();

    let mut cursor = QueryCursor::new();

    // Group captures by their parent import_statement node
    // Key: (start_byte, end_byte) of the import_statement
    let mut static_imports: FxHashMap<(usize, usize), ImportBuilder> = FxHashMap::default();
    let mut dynamic_imports: SmallVec<[ImportInfo; 8]> = smallvec![];

    // Execute the query and iterate over matches using StreamingIterator
    cursor.set_max_start_depth(None);
    let mut matches = cursor.matches(query, root, source_bytes);

    while let Some(match_) = matches.next() {
        for capture in match_.captures {
            let capture_index = capture.index;
            let node = capture.node;

            match capture_index {
                idx if idx == CAPTURE_IMPORT_STATEMENT => {
                    // Initialize an entry for this import statement
                    let key = (node.start_byte(), node.end_byte());
                    static_imports
                        .entry(key)
                        .or_insert_with(|| ImportBuilder::new(node, source_bytes));
                }
                idx if idx == CAPTURE_IMPORT_SOURCE => {
                    // Find the parent import_statement and set its source
                    if let Some(parent) = find_import_statement_parent(node) {
                        let key = (parent.start_byte(), parent.end_byte());
                        let builder = static_imports
                            .entry(key)
                            .or_insert_with(|| ImportBuilder::new(parent, source_bytes));
                        builder.set_source(node, source_bytes);
                    }
                }
                idx if idx == CAPTURE_IMPORT_NAMED_NAME => {
                    // Add a named import
                    if let Some(parent) = find_import_statement_parent(node) {
                        let key = (parent.start_byte(), parent.end_byte());
                        let builder = static_imports
                            .entry(key)
                            .or_insert_with(|| ImportBuilder::new(parent, source_bytes));
                        builder.add_named_import(node, source_bytes);
                    }
                }
                idx if idx == CAPTURE_IMPORT_DEFAULT_NAME => {
                    // Set as default import
                    if let Some(parent) = find_import_statement_parent(node) {
                        let key = (parent.start_byte(), parent.end_byte());
                        let builder = static_imports
                            .entry(key)
                            .or_insert_with(|| ImportBuilder::new(parent, source_bytes));
                        builder.set_default_import(node, source_bytes);
                    }
                }
                idx if idx == CAPTURE_IMPORT_NAMESPACE_NAME => {
                    // Set as namespace import
                    if let Some(parent) = find_import_statement_parent(node) {
                        let key = (parent.start_byte(), parent.end_byte());
                        let builder = static_imports
                            .entry(key)
                            .or_insert_with(|| ImportBuilder::new(parent, source_bytes));
                        builder.set_namespace_import(node, source_bytes);
                    }
                }
                idx if idx == CAPTURE_IMPORT_DYNAMIC_SOURCE => {
                    // Dynamic import - create directly
                    if let Some(import) = create_dynamic_import(node, source_bytes) {
                        dynamic_imports.push(import);
                    }
                }
                _ => {}
            }
        }
    }

    // Build final import list
    let mut imports: SmallVec<[ImportInfo; 8]> = static_imports
        .into_values()
        .filter_map(ImportBuilder::build)
        .collect();

    imports.extend(dynamic_imports);

    // Sort by source location for consistent ordering
    imports.sort_by_key(|i| (i.location.line, i.location.column));

    imports
}

/// Builder for constructing an [`ImportInfo`] from captured nodes.
struct ImportBuilder {
    /// Source path (the string after `from`)
    source_path: Option<String>,
    /// Imported names
    names: SmallVec<[String; 4]>,
    /// The kind of import detected
    kind: Option<ImportKind>,
    /// Source location of the import statement
    location: SourceLocation,
    /// Whether this is a type-only import
    is_type_only: bool,
}

impl ImportBuilder {
    /// Creates a new import builder from an `import_statement` node.
    fn new(statement_node: Node<'_>, source: &[u8]) -> Self {
        let location = node_to_location(statement_node);
        let is_type_only = check_type_only(statement_node, source);

        Self {
            source_path: None,
            names: smallvec![],
            kind: None,
            location,
            is_type_only,
        }
    }

    /// Sets the source path from a string node.
    fn set_source(&mut self, node: Node<'_>, source: &[u8]) {
        if let Some(text) = node_text(node, source) {
            self.source_path = Some(text.to_owned());
        }
    }

    /// Adds a named import identifier.
    fn add_named_import(&mut self, node: Node<'_>, source: &[u8]) {
        if let Some(text) = node_text(node, source) {
            self.names.push(text.to_owned());
            // Set kind to Named if not already set to something more specific
            if self.kind.is_none() {
                self.kind = Some(ImportKind::Named);
            }
        }
    }

    /// Sets this as a default import.
    fn set_default_import(&mut self, node: Node<'_>, source: &[u8]) {
        if let Some(text) = node_text(node, source) {
            self.names.push(text.to_owned());
            self.kind = Some(ImportKind::Default);
        }
    }

    /// Sets this as a namespace import.
    fn set_namespace_import(&mut self, node: Node<'_>, source: &[u8]) {
        if let Some(text) = node_text(node, source) {
            self.names.push(text.to_owned());
            self.kind = Some(ImportKind::Namespace);
        }
    }

    /// Builds the final [`ImportInfo`], returning `None` if incomplete.
    fn build(self) -> Option<ImportInfo> {
        let path = self.source_path?;
        let source = detect_model_source(&path);

        // Determine the final import kind
        let kind = if self.is_type_only {
            ImportKind::TypeOnly
        } else if let Some(k) = self.kind {
            k
        } else if self.names.is_empty() {
            ImportKind::SideEffect
        } else {
            ImportKind::Named
        };

        Some(ImportInfo::new(path, kind, self.names, source, self.location))
    }
}

/// Creates an [`ImportInfo`] for a dynamic import.
fn create_dynamic_import(source_node: Node<'_>, source: &[u8]) -> Option<ImportInfo> {
    let path = node_text(source_node, source)?.to_owned();
    let model_source = detect_model_source(&path);
    let location = node_to_location(source_node);

    Some(ImportInfo::new(
        path,
        ImportKind::Dynamic,
        smallvec![],
        model_source,
        location,
    ))
}

/// Finds the parent `import_statement` node for a given node.
fn find_import_statement_parent(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = Some(node);
    while let Some(n) = current {
        if n.kind() == "import_statement" {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// Checks if an `import_statement` is a type-only import.
///
/// Type-only imports start with `import type`.
fn check_type_only(statement_node: Node<'_>, source: &[u8]) -> bool {
    // Check if the import statement text starts with "import type"
    if let Some(text) = node_text(statement_node, source) {
        return text.trim_start().starts_with("import type");
    }

    // Alternative: check for a child with kind "type"
    let mut cursor = statement_node.walk();
    for child in statement_node.children(&mut cursor) {
        if child.kind() == "type" {
            return true;
        }
    }

    false
}

/// Extracts text from a node.
fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let start = node.start_byte();
    let end = node.end_byte();
    std::str::from_utf8(source.get(start..end)?).ok()
}

/// Converts a node's position to a [`SourceLocation`].
///
/// # Note
///
/// The casts from `usize` to `u32` are safe because source files
/// are limited to 4GB, which fits in `u32`.
#[allow(clippy::cast_possible_truncation)]
fn node_to_location(node: Node<'_>) -> SourceLocation {
    let start = node.start_position();
    SourceLocation::new(
        start.row as u32 + 1, // Convert 0-indexed to 1-indexed
        start.column as u32,
        node.start_byte() as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::{Language, Parser};

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
        Query::new(&language, crate::queries::IMPORT_QUERY).expect("Query should compile")
    }

    #[test]
    fn test_extract_named_imports() {
        let source = r#"import { Foo, Bar } from '../shared/models/foo';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 1);

        let import = &imports[0];
        assert_eq!(import.path, "'../shared/models/foo'");
        assert_eq!(import.kind, ImportKind::Named);
        assert_eq!(import.names.len(), 2);
        assert!(import.names.contains(&"Foo".to_owned()));
        assert!(import.names.contains(&"Bar".to_owned()));
        assert!(import.is_legacy_import());
    }

    #[test]
    fn test_extract_default_import() {
        let source = r#"import Foo from '../shared_2023/models/foo';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 1);

        let import = &imports[0];
        assert_eq!(import.kind, ImportKind::Default);
        assert_eq!(import.names.len(), 1);
        assert_eq!(import.names[0], "Foo");
        assert!(!import.is_legacy_import());
    }

    #[test]
    fn test_extract_namespace_import() {
        let source = r#"import * as Models from '../shared/models';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 1);

        let import = &imports[0];
        assert_eq!(import.kind, ImportKind::Namespace);
        assert_eq!(import.names.len(), 1);
        assert_eq!(import.names[0], "Models");
    }

    #[test]
    fn test_extract_side_effect_import() {
        let source = r#"import '../shared/polyfills';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 1);

        let import = &imports[0];
        assert_eq!(import.kind, ImportKind::SideEffect);
        assert!(import.names.is_empty());
    }

    #[test]
    fn test_extract_type_only_import() {
        let source = r#"import type { FooModel } from '../shared/interfaces';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 1);

        let import = &imports[0];
        assert_eq!(import.kind, ImportKind::TypeOnly);
    }

    #[test]
    fn test_extract_dynamic_import() {
        let source = r#"const mod = await import('../shared/models/foo');"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 1);

        let import = &imports[0];
        assert_eq!(import.kind, ImportKind::Dynamic);
        assert!(import.is_legacy_import());
    }

    #[test]
    fn test_extract_multiple_imports() {
        let source = r#"
import { Foo } from '../shared/models/foo';
import { Bar } from '../shared_2023/models/bar';
import '@angular/core';
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 3);

        // Check that we have both legacy and new imports
        let legacy_count = imports.iter().filter(|i| i.is_legacy_import()).count();
        let new_count = imports
            .iter()
            .filter(|i| i.source.is_some_and(|s| !s.is_legacy()))
            .count();

        assert_eq!(legacy_count, 1);
        assert_eq!(new_count, 1);
    }

    #[test]
    fn test_non_shared_imports() {
        let source = r#"import { Component } from '@angular/core';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 1);
        assert!(!imports[0].is_model_import());
    }

    #[test]
    fn test_source_location() {
        let source = r#"import { Foo } from './foo';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports(&tree, source, &query);
        assert_eq!(imports.len(), 1);

        let loc = &imports[0].location;
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 0);
        assert_eq!(loc.byte_offset, 0);
    }
}
