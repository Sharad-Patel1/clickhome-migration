//! Error types for the ch-ts-parser crate.
//!
//! This module provides the [`ParseError`] type for errors that can occur
//! during TypeScript parsing and import extraction.

/// Errors that can occur during TypeScript parsing.
///
/// These errors cover initialization failures, query compilation errors,
/// and parse failures.
///
/// # Examples
///
/// ```
/// use ch_ts_parser::ParseError;
///
/// fn handle_error(err: ParseError) {
///     match err {
///         ParseError::ParserInit => eprintln!("Failed to initialize parser"),
///         ParseError::LanguageInit => eprintln!("Failed to set TypeScript language"),
///         ParseError::QueryCompile { offset, .. } => {
///             eprintln!("Query compilation failed at offset {offset}");
///         }
///         ParseError::Parse => eprintln!("Failed to parse source code"),
///     }
/// }
/// ```
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Failed to create a new tree-sitter parser.
    #[error("failed to initialize tree-sitter parser")]
    ParserInit,

    /// Failed to set the TypeScript language on the parser.
    #[error("failed to set TypeScript language")]
    LanguageInit,

    /// Failed to compile a tree-sitter query.
    ///
    /// Contains the byte offset where the error occurred and the error kind.
    #[error("failed to compile query at offset {offset}: {kind:?}")]
    QueryCompile {
        /// The byte offset in the query string where the error occurred.
        offset: usize,
        /// The kind of query error.
        kind: tree_sitter::QueryError,
    },

    /// Failed to parse the source code.
    ///
    /// This typically indicates the parser ran out of memory or was cancelled.
    #[error("failed to parse source code")]
    Parse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_init_display() {
        let err = ParseError::ParserInit;
        assert_eq!(err.to_string(), "failed to initialize tree-sitter parser");
    }

    #[test]
    fn test_language_init_display() {
        let err = ParseError::LanguageInit;
        assert_eq!(err.to_string(), "failed to set TypeScript language");
    }

    #[test]
    fn test_parse_display() {
        let err = ParseError::Parse;
        assert_eq!(err.to_string(), "failed to parse source code");
    }
}
