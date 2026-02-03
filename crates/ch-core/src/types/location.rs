//! Source location types for tracking positions in source files.
//!
//! This module provides the [`SourceLocation`] type for representing positions
//! within TypeScript source files.

use serde::{Deserialize, Serialize};

/// A position within a source file.
///
/// Represents a specific location in a TypeScript file, useful for
/// tracking import statements and model references.
///
/// # Field Conventions
///
/// - `line` is 1-indexed (first line is line 1)
/// - `column` is 0-indexed (first character is column 0)
/// - `byte_offset` is the absolute byte position from the start of the file
///
/// # Examples
///
/// ```
/// use ch_core::SourceLocation;
///
/// let loc = SourceLocation {
///     line: 10,
///     column: 5,
///     byte_offset: 245,
/// };
///
/// assert_eq!(loc.line, 10);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct SourceLocation {
    /// Line number (1-indexed).
    pub line: u32,

    /// Column number (0-indexed, UTF-8 byte offset within the line).
    pub column: u32,

    /// Absolute byte offset from the start of the file.
    pub byte_offset: u32,
}

impl SourceLocation {
    /// Creates a new source location.
    ///
    /// # Arguments
    ///
    /// * `line` - Line number (1-indexed)
    /// * `column` - Column number (0-indexed)
    /// * `byte_offset` - Absolute byte offset
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::SourceLocation;
    ///
    /// let loc = SourceLocation::new(10, 5, 245);
    /// assert_eq!(loc.line, 10);
    /// assert_eq!(loc.column, 5);
    /// assert_eq!(loc.byte_offset, 245);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(line: u32, column: u32, byte_offset: u32) -> Self {
        Self {
            line,
            column,
            byte_offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_location_new() {
        let loc = SourceLocation::new(10, 5, 245);
        assert_eq!(loc.line, 10);
        assert_eq!(loc.column, 5);
        assert_eq!(loc.byte_offset, 245);
    }

    #[test]
    fn test_source_location_default() {
        let loc = SourceLocation::default();
        assert_eq!(loc.line, 0);
        assert_eq!(loc.column, 0);
        assert_eq!(loc.byte_offset, 0);
    }

    #[test]
    fn test_source_location_serialization() {
        let loc = SourceLocation::new(10, 5, 245);
        let json = serde_json::to_string(&loc).unwrap();
        let parsed: SourceLocation = serde_json::from_str(&json).unwrap();
        assert_eq!(loc, parsed);
    }

    #[test]
    fn test_source_location_copy() {
        let loc1 = SourceLocation::new(1, 2, 3);
        let loc2 = loc1; // Copy
        assert_eq!(loc1, loc2);
    }
}
