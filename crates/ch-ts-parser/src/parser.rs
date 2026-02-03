//! TypeScript parser management using tree-sitter.
//!
//! This module provides the [`TsParser`] struct for parsing TypeScript files
//! and extracting import information.

use ch_core::ImportInfo;
use smallvec::SmallVec;
use tree_sitter::{InputEdit, Language, Parser, Query, Tree};

use crate::error::ParseError;
use crate::import::extract_imports;
use crate::queries::{get_tsx_import_query, get_typescript_import_query};

/// Indicates whether the parser is configured for TypeScript or TSX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserKind {
    TypeScript,
    Tsx,
}

/// Result of parsing a TypeScript file.
///
/// Contains the extracted imports and the syntax tree, which can be used
/// for incremental re-parsing when the file changes.
///
/// # Examples
///
/// ```ignore
/// let result = parser.parse(source)?;
/// println!("Found {} imports", result.imports.len());
///
/// // Later, when the file changes:
/// let new_result = parser.parse_incremental(new_source, &result.tree, &edit)?;
/// ```
#[derive(Debug)]
pub struct ParseResult {
    /// All import statements detected in the file.
    ///
    /// Uses `SmallVec<[ImportInfo; 8]>` to avoid heap allocation for
    /// typical files with 8 or fewer imports.
    pub imports: SmallVec<[ImportInfo; 8]>,

    /// The syntax tree from parsing.
    ///
    /// Retained for incremental re-parsing. When a file changes, pass this
    /// tree to [`TsParser::parse_incremental`] along with the edit information
    /// for efficient re-parsing.
    pub tree: Tree,
}

/// TypeScript parser for extracting imports from source files.
///
/// Wraps a tree-sitter parser configured for TypeScript. The parser can be
/// reused for multiple files to avoid repeated initialization.
///
/// # Thread Safety
///
/// `TsParser` is `Send` but not `Sync`. For parallel scanning with rayon,
/// either:
/// - Create one parser per thread using `thread_local!`
/// - Create a new parser for each parallel task
///
/// The underlying tree-sitter [`Query`] is thread-safe and shared across
/// all parser instances.
///
/// # Examples
///
/// ```
/// use ch_ts_parser::TsParser;
///
/// let mut parser = TsParser::new()?;
/// let source = r#"import { Foo } from '../shared/models/foo';"#;
/// let result = parser.parse(source)?;
///
/// for import in &result.imports {
///     if import.is_legacy_import() {
///         println!("Legacy import: {}", import.path);
///     }
/// }
/// # Ok::<(), ch_ts_parser::ParseError>(())
/// ```
pub struct TsParser {
    /// The underlying tree-sitter parser.
    parser: Parser,
    /// The TypeScript language for the parser.
    language: Language,
    /// Whether this is a TypeScript or TSX parser.
    kind: ParserKind,
}

impl TsParser {
    /// Creates a new TypeScript parser.
    ///
    /// Initializes a tree-sitter parser configured for TypeScript.
    /// The import query is compiled lazily on first use.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::LanguageInit`] if the TypeScript language
    /// cannot be set on the parser.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_ts_parser::TsParser;
    ///
    /// let parser = TsParser::new()?;
    /// # Ok::<(), ch_ts_parser::ParseError>(())
    /// ```
    pub fn new() -> Result<Self, ParseError> {
        let mut parser = Parser::new();
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();

        parser
            .set_language(&language)
            .map_err(|_| ParseError::LanguageInit)?;

        Ok(Self {
            parser,
            language,
            kind: ParserKind::TypeScript,
        })
    }

    /// Creates a new TypeScript TSX parser.
    ///
    /// Initializes a tree-sitter parser configured for TSX (TypeScript with JSX).
    /// Use this for `.tsx` files.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::LanguageInit`] if the TSX language
    /// cannot be set on the parser.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_ts_parser::TsParser;
    ///
    /// let parser = TsParser::new_tsx()?;
    /// # Ok::<(), ch_ts_parser::ParseError>(())
    /// ```
    pub fn new_tsx() -> Result<Self, ParseError> {
        let mut parser = Parser::new();
        let language: Language = tree_sitter_typescript::LANGUAGE_TSX.into();

        parser
            .set_language(&language)
            .map_err(|_| ParseError::LanguageInit)?;

        Ok(Self {
            parser,
            language,
            kind: ParserKind::Tsx,
        })
    }

    /// Returns the appropriate import query for this parser's language.
    fn get_query(&self) -> Result<&'static Query, ParseError> {
        match self.kind {
            ParserKind::TypeScript => get_typescript_import_query(),
            ParserKind::Tsx => get_tsx_import_query(),
        }
    }

    /// Parses TypeScript source code and extracts imports.
    ///
    /// This is a fresh parse that creates a new syntax tree. For re-parsing
    /// after edits, use [`parse_incremental`](Self::parse_incremental) instead.
    ///
    /// # Arguments
    ///
    /// * `source` - The TypeScript source code to parse
    ///
    /// # Returns
    ///
    /// A [`ParseResult`] containing the extracted imports and the syntax tree.
    ///
    /// # Errors
    ///
    /// - Returns [`ParseError::Parse`] if parsing fails
    /// - Returns [`ParseError::QueryCompile`] if the import query fails to compile
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_ts_parser::TsParser;
    ///
    /// let mut parser = TsParser::new()?;
    /// let source = r#"
    ///     import { Foo } from '../shared/models/foo';
    ///     import { Bar } from '../shared_2023/models/bar';
    /// "#;
    ///
    /// let result = parser.parse(source)?;
    /// assert_eq!(result.imports.len(), 2);
    /// # Ok::<(), ch_ts_parser::ParseError>(())
    /// ```
    pub fn parse(&mut self, source: &str) -> Result<ParseResult, ParseError> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or(ParseError::Parse)?;

        let query = self.get_query()?;
        let imports = extract_imports(&tree, source, query);

        Ok(ParseResult { imports, tree })
    }

    /// Incrementally re-parses TypeScript source after an edit.
    ///
    /// This is more efficient than [`parse`](Self::parse) when making small
    /// changes to a file, as tree-sitter can reuse unchanged portions of
    /// the syntax tree.
    ///
    /// # Arguments
    ///
    /// * `source` - The new source code after the edit
    /// * `old_tree` - The syntax tree from the previous parse
    /// * `edit` - Information about what changed
    ///
    /// # Returns
    ///
    /// A new [`ParseResult`] with updated imports and syntax tree.
    ///
    /// # Errors
    ///
    /// - Returns [`ParseError::Parse`] if parsing fails
    /// - Returns [`ParseError::QueryCompile`] if the import query fails to compile
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use ch_ts_parser::TsParser;
    /// use tree_sitter::{InputEdit, Point};
    ///
    /// let mut parser = TsParser::new()?;
    ///
    /// // Initial parse
    /// let source = "import { Foo } from './foo';";
    /// let result = parser.parse(source)?;
    ///
    /// // After editing the file
    /// let new_source = "import { Foo, Bar } from './foo';";
    /// let edit = InputEdit {
    ///     start_byte: 14,
    ///     old_end_byte: 14,
    ///     new_end_byte: 19,
    ///     start_position: Point::new(0, 14),
    ///     old_end_position: Point::new(0, 14),
    ///     new_end_position: Point::new(0, 19),
    /// };
    ///
    /// let new_result = parser.parse_incremental(new_source, &result.tree, &edit)?;
    /// ```
    pub fn parse_incremental(
        &mut self,
        source: &str,
        old_tree: &Tree,
        edit: &InputEdit,
    ) -> Result<ParseResult, ParseError> {
        // Clone and edit the old tree
        let mut edited_tree = old_tree.clone();
        edited_tree.edit(edit);

        // Parse with the edited tree as a hint
        let tree = self
            .parser
            .parse(source, Some(&edited_tree))
            .ok_or(ParseError::Parse)?;

        let query = self.get_query()?;
        let imports = extract_imports(&tree, source, query);

        Ok(ParseResult { imports, tree })
    }

    /// Returns the tree-sitter language used by this parser.
    ///
    /// This is useful when you need to create queries compatible with
    /// this parser's language.
    #[inline]
    pub fn language(&self) -> &Language {
        &self.language
    }
}

impl std::fmt::Debug for TsParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsParser")
            .field("language", &"TypeScript")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_core::{ImportKind, ModelSource};
    use tree_sitter::Point;

    #[test]
    fn test_parser_new() {
        let parser = TsParser::new();
        assert!(parser.is_ok());
    }

    #[test]
    fn test_parser_new_tsx() {
        let parser = TsParser::new_tsx();
        assert!(parser.is_ok());
    }

    #[test]
    fn test_parse_simple() {
        let mut parser = TsParser::new().expect("Parser creation failed");
        let source = r#"import { Foo } from '../shared/models/foo';"#;

        let result = parser.parse(source).expect("Parse failed");
        assert_eq!(result.imports.len(), 1);
        assert!(result.imports[0].is_legacy_import());
    }

    #[test]
    fn test_parse_multiple_imports() {
        let mut parser = TsParser::new().expect("Parser creation failed");
        let source = r#"
import { Foo } from '../shared/models/foo';
import { Bar } from '../shared_2023/models/bar';
import { Component } from '@angular/core';
"#;

        let result = parser.parse(source).expect("Parse failed");
        assert_eq!(result.imports.len(), 3);

        // Verify we correctly identified sources
        let legacy = result.imports.iter().find(|i| i.is_legacy_import());
        assert!(legacy.is_some());
        assert!(legacy
            .expect("Should have legacy import")
            .path
            .contains("shared/models"));

        let new = result
            .imports
            .iter()
            .find(|i| i.source == Some(ModelSource::Shared2023));
        assert!(new.is_some());
    }

    #[test]
    fn test_parse_all_import_kinds() {
        let mut parser = TsParser::new().expect("Parser creation failed");
        let source = r#"
import { Named } from './named';
import Default from './default';
import * as Namespace from './namespace';
import './side-effect';
import type { TypeOnly } from './type-only';
"#;

        let result = parser.parse(source).expect("Parse failed");
        assert_eq!(result.imports.len(), 5);

        let kinds: Vec<_> = result.imports.iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&ImportKind::Named));
        assert!(kinds.contains(&ImportKind::Default));
        assert!(kinds.contains(&ImportKind::Namespace));
        assert!(kinds.contains(&ImportKind::SideEffect));
        assert!(kinds.contains(&ImportKind::TypeOnly));
    }

    #[test]
    fn test_parse_dynamic_import() {
        let mut parser = TsParser::new().expect("Parser creation failed");
        let source = r#"const mod = await import('../shared/models/foo');"#;

        let result = parser.parse(source).expect("Parse failed");
        assert_eq!(result.imports.len(), 1);
        assert_eq!(result.imports[0].kind, ImportKind::Dynamic);
        assert!(result.imports[0].is_legacy_import());
    }

    #[test]
    fn test_parse_incremental() {
        let mut parser = TsParser::new().expect("Parser creation failed");

        // Initial parse
        let source1 = "import { Foo } from './foo';";
        let result1 = parser.parse(source1).expect("Parse failed");
        assert_eq!(result1.imports.len(), 1);
        assert_eq!(result1.imports[0].names.len(), 1);

        // Edit: add Bar to the import
        let source2 = "import { Foo, Bar } from './foo';";
        let edit = InputEdit {
            start_byte: 13,
            old_end_byte: 13,
            new_end_byte: 18,
            start_position: Point::new(0, 13),
            old_end_position: Point::new(0, 13),
            new_end_position: Point::new(0, 18),
        };

        let result2 = parser
            .parse_incremental(source2, &result1.tree, &edit)
            .expect("Incremental parse failed");

        assert_eq!(result2.imports.len(), 1);
        assert_eq!(result2.imports[0].names.len(), 2);
    }

    #[test]
    fn test_parse_empty_source() {
        let mut parser = TsParser::new().expect("Parser creation failed");
        let result = parser.parse("").expect("Parse failed");
        assert!(result.imports.is_empty());
    }

    #[test]
    fn test_parse_no_imports() {
        let mut parser = TsParser::new().expect("Parser creation failed");
        let source = r#"
const x = 1;
function foo() { return x; }
"#;
        let result = parser.parse(source).expect("Parse failed");
        assert!(result.imports.is_empty());
    }

    #[test]
    fn test_parse_tsx() {
        let mut parser = TsParser::new_tsx().expect("Parser creation failed");
        let source = r#"
import React from 'react';
import { Foo } from '../shared/models/foo';

const App = () => <div>Hello</div>;
"#;

        let result = parser.parse(source).expect("Parse failed");
        assert_eq!(result.imports.len(), 2);
    }

    #[test]
    fn test_parser_debug() {
        let parser = TsParser::new().expect("Parser creation failed");
        let debug = format!("{parser:?}");
        assert!(debug.contains("TsParser"));
        assert!(debug.contains("TypeScript"));
    }
}
