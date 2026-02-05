//! Import extraction from TypeScript source using tree-sitter queries.
//!
//! This module provides functions to extract import information from parsed
//! TypeScript syntax trees. Two APIs are available:
//!
//! - [`extract_imports`]: Convenience function returning owned [`ImportInfo`]
//! - [`extract_imports_arena`]: Arena-backed version for high-performance parsing
//!
//! # Arena Allocation
//!
//! For parallel scanning of many files, use [`extract_imports_arena`] with a
//! [`bumpalo::Bump`] arena (or [`bumpalo_herd::Herd`] for per-thread arenas).
//! This reduces heap allocations during the extraction loop.
//!
//! ```ignore
//! use bumpalo::Bump;
//! use ch_ts_parser::import::extract_imports_arena;
//!
//! let arena = Bump::new();
//! let bump_imports = extract_imports_arena(&arena, &tree, source, query);
//!
//! // Convert to owned when needed
//! let imports: Vec<ImportInfo> = bump_imports.into_iter().map(Into::into).collect();
//! ```

use bumpalo::Bump;
use ch_core::{FxHashMap, ImportInfo, SourceLocation};
use smallvec::{smallvec, SmallVec};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, Tree};

use crate::arena::{create_dynamic_bump_import, BumpImportBuilder, BumpImportInfo, StringInterner};
use crate::queries::{
    CAPTURE_IMPORT_DEFAULT_NAME, CAPTURE_IMPORT_DYNAMIC_SOURCE, CAPTURE_IMPORT_NAMED_NAME,
    CAPTURE_IMPORT_NAMESPACE_NAME, CAPTURE_IMPORT_SOURCE, CAPTURE_IMPORT_STATEMENT,
};
use crate::source::detect_model_source;

/// Extracts all imports from a parsed TypeScript syntax tree.
///
/// This is a convenience wrapper around [`extract_imports_arena`] that manages
/// an internal arena and converts results to owned [`ImportInfo`].
///
/// For high-performance parallel scanning, use [`extract_imports_arena`] directly
/// with a shared [`bumpalo_herd::Herd`].
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
    // Create an internal arena for this extraction
    let arena = Bump::new();

    // Extract using arena-backed implementation
    let bump_imports = extract_imports_arena(&arena, tree, source, query);

    // Convert to owned and collect
    let mut imports: SmallVec<[ImportInfo; 8]> = bump_imports
        .into_iter()
        .map(BumpImportInfo::into_owned)
        .collect();

    // Sort by source location for consistent ordering
    imports.sort_by_key(|i| (i.location.line, i.location.column));

    imports
}

/// Extracts all imports using arena allocation for zero-copy during extraction.
///
/// This is the high-performance version of import extraction. All string data
/// is allocated in the provided [`Bump`] arena, eliminating per-string heap
/// allocations during the extraction loop.
///
/// # Performance
///
/// - Strings are interned within the arena, so repeated paths share memory
/// - No heap allocations during the extraction loop (only arena grows)
/// - Results must be converted to owned before the arena is dropped
///
/// # Arguments
///
/// * `arena` - The bump arena for string allocation
/// * `tree` - The parsed syntax tree
/// * `source` - The original source code (needed to extract text from nodes)
/// * `query` - The pre-compiled import query
///
/// # Returns
///
/// A vector of [`BumpImportInfo`] with string data borrowed from the arena.
/// Convert to [`ImportInfo`] using `.into()` or `.into_owned()` when the data
/// needs to outlive the arena.
///
/// # Examples
///
/// ```ignore
/// use bumpalo::Bump;
/// use ch_ts_parser::import::extract_imports_arena;
///
/// let arena = Bump::new();
/// let bump_imports = extract_imports_arena(&arena, &tree, source, query);
///
/// // Process while arena is alive
/// for import in &bump_imports {
///     if import.is_legacy_import() {
///         println!("Legacy: {}", import.path);
///     }
/// }
///
/// // Convert to owned when needed
/// let owned: Vec<ImportInfo> = bump_imports.into_iter().map(Into::into).collect();
/// ```
pub fn extract_imports_arena<'bump>(
    arena: &'bump Bump,
    tree: &Tree,
    source: &str,
    query: &Query,
) -> SmallVec<[BumpImportInfo<'bump>; 8]> {
    let source_bytes = source.as_bytes();
    let root = tree.root_node();

    // Create string interner for path deduplication
    let mut interner = StringInterner::new(arena);

    let mut cursor = QueryCursor::new();

    // Group captures by their parent import_statement node
    // Key: (start_byte, end_byte) of the import_statement
    let mut static_imports: FxHashMap<(usize, usize), BumpImportBuilder<'bump>> =
        FxHashMap::default();
    let mut dynamic_imports: SmallVec<[BumpImportInfo<'bump>; 8]> = smallvec![];

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
                    static_imports.entry(key).or_insert_with(|| {
                        let location = node_to_location(node);
                        let is_type_only = check_type_only(node, source_bytes);
                        BumpImportBuilder::new(location, is_type_only)
                    });
                }
                idx if idx == CAPTURE_IMPORT_SOURCE => {
                    // Find the parent import_statement and set its source
                    if let Some(parent) = find_import_statement_parent(node) {
                        let key = (parent.start_byte(), parent.end_byte());
                        let builder = static_imports.entry(key).or_insert_with(|| {
                            let location = node_to_location(parent);
                            let is_type_only = check_type_only(parent, source_bytes);
                            BumpImportBuilder::new(location, is_type_only)
                        });
                        if let Some(text) = node_text(node, source_bytes) {
                            let interned = interner.intern(text);
                            builder.set_source(interned);
                        }
                    }
                }
                idx if idx == CAPTURE_IMPORT_NAMED_NAME => {
                    // Add a named import
                    if let Some(parent) = find_import_statement_parent(node) {
                        let key = (parent.start_byte(), parent.end_byte());
                        let builder = static_imports.entry(key).or_insert_with(|| {
                            let location = node_to_location(parent);
                            let is_type_only = check_type_only(parent, source_bytes);
                            BumpImportBuilder::new(location, is_type_only)
                        });
                        if let Some(text) = node_text(node, source_bytes) {
                            let interned = interner.intern(text);
                            builder.add_named_import(interned);
                        }
                    }
                }
                idx if idx == CAPTURE_IMPORT_DEFAULT_NAME => {
                    // Set as default import
                    if let Some(parent) = find_import_statement_parent(node) {
                        let key = (parent.start_byte(), parent.end_byte());
                        let builder = static_imports.entry(key).or_insert_with(|| {
                            let location = node_to_location(parent);
                            let is_type_only = check_type_only(parent, source_bytes);
                            BumpImportBuilder::new(location, is_type_only)
                        });
                        if let Some(text) = node_text(node, source_bytes) {
                            let interned = interner.intern(text);
                            builder.set_default_import(interned);
                        }
                    }
                }
                idx if idx == CAPTURE_IMPORT_NAMESPACE_NAME => {
                    // Set as namespace import
                    if let Some(parent) = find_import_statement_parent(node) {
                        let key = (parent.start_byte(), parent.end_byte());
                        let builder = static_imports.entry(key).or_insert_with(|| {
                            let location = node_to_location(parent);
                            let is_type_only = check_type_only(parent, source_bytes);
                            BumpImportBuilder::new(location, is_type_only)
                        });
                        if let Some(text) = node_text(node, source_bytes) {
                            let interned = interner.intern(text);
                            builder.set_namespace_import(interned);
                        }
                    }
                }
                idx if idx == CAPTURE_IMPORT_DYNAMIC_SOURCE => {
                    // Dynamic import - create directly
                    if let Some(text) = node_text(node, source_bytes) {
                        let path = interner.intern(text);
                        let model_source = detect_model_source(path.as_str());
                        let location = node_to_location(node);
                        dynamic_imports.push(create_dynamic_bump_import(
                            path,
                            model_source,
                            location,
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    // Build final import list
    let mut imports: SmallVec<[BumpImportInfo<'bump>; 8]> = static_imports
        .into_values()
        .filter_map(|builder| builder.build(detect_model_source))
        .collect();

    imports.extend(dynamic_imports);

    // Sort by source location for consistent ordering
    imports.sort_by_key(|i| (i.location.line, i.location.column));

    imports
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
    use ch_core::{ImportKind, ModelSource};
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

    // =========================================================================
    // Tests for extract_imports (owned API)
    // =========================================================================

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

    // =========================================================================
    // Tests for extract_imports_arena (arena API)
    // =========================================================================

    #[test]
    fn test_arena_extract_named_imports() {
        let arena = Bump::new();
        let source = r#"import { Foo, Bar } from '../shared/models/foo';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports_arena(&arena, &tree, source, &query);
        assert_eq!(imports.len(), 1);

        let import = &imports[0];
        assert_eq!(import.path.as_str(), "'../shared/models/foo'");
        assert_eq!(import.kind, ImportKind::Named);
        assert_eq!(import.names.len(), 2);
        assert!(import.is_legacy_import());
    }

    #[test]
    fn test_arena_extract_converts_to_owned() {
        let arena = Bump::new();
        let source = r#"import { Foo } from '../shared/models/foo';"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let bump_imports = extract_imports_arena(&arena, &tree, source, &query);
        assert_eq!(bump_imports.len(), 1);

        // Convert to owned
        let owned: ImportInfo = bump_imports
            .into_iter()
            .next()
            .expect("should have one")
            .into();
        assert_eq!(owned.path, "'../shared/models/foo'");
        assert_eq!(owned.names[0], "Foo");
    }

    #[test]
    fn test_arena_string_interning() {
        let arena = Bump::new();
        // Multiple imports from the same path - should be deduplicated
        let source = r#"
import { Foo } from '../shared/models/foo';
import { Bar } from '../shared/models/foo';
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports_arena(&arena, &tree, source, &query);
        assert_eq!(imports.len(), 2);

        // Both imports should have paths pointing to the same interned string
        let path1 = imports[0].path.as_str();
        let path2 = imports[1].path.as_str();
        assert!(
            std::ptr::eq(path1, path2),
            "Paths should be interned (same pointer)"
        );
    }

    #[test]
    fn test_arena_all_import_kinds() {
        let arena = Bump::new();
        let source = r#"
import { Named } from './named';
import Default from './default';
import * as Namespace from './namespace';
import './side-effect';
import type { TypeOnly } from './type-only';
const dyn = await import('./dynamic');
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports_arena(&arena, &tree, source, &query);
        assert_eq!(imports.len(), 6);

        let kinds: Vec<_> = imports.iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&ImportKind::Named));
        assert!(kinds.contains(&ImportKind::Default));
        assert!(kinds.contains(&ImportKind::Namespace));
        assert!(kinds.contains(&ImportKind::SideEffect));
        assert!(kinds.contains(&ImportKind::TypeOnly));
        assert!(kinds.contains(&ImportKind::Dynamic));
    }

    #[test]
    fn test_arena_model_source_detection() {
        let arena = Bump::new();
        let source = r#"
import { Legacy } from '../shared/models/foo';
import { New } from '../shared_2023/models/bar';
import { Other } from '@angular/core';
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports_arena(&arena, &tree, source, &query);
        assert_eq!(imports.len(), 3);

        let legacy = imports
            .iter()
            .find(|i| i.path.contains("shared/models"))
            .expect("should have legacy");
        let new = imports
            .iter()
            .find(|i| i.path.contains("shared_2023"))
            .expect("should have new");
        let other = imports
            .iter()
            .find(|i| i.path.contains("angular"))
            .expect("should have other");

        assert_eq!(legacy.source, Some(ModelSource::SharedLegacy));
        assert_eq!(new.source, Some(ModelSource::Shared2023));
        assert_eq!(other.source, None);
    }

    #[test]
    fn test_arena_empty_source() {
        let arena = Bump::new();
        let source = "";
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports_arena(&arena, &tree, source, &query);
        assert!(imports.is_empty());
    }

    #[test]
    fn test_arena_no_imports() {
        let arena = Bump::new();
        let source = r#"
const x = 1;
function foo() { return x; }
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse failed");
        let query = create_query();

        let imports = extract_imports_arena(&arena, &tree, source, &query);
        assert!(imports.is_empty());
    }
}
