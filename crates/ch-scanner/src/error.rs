//! Error types for the ch-scanner crate.
//!
//! This module provides the [`ScanError`] type for errors that can occur
//! during directory traversal and file analysis.

use camino::Utf8PathBuf;

/// Errors that can occur during scanning operations.
///
/// These errors cover directory traversal failures, file I/O errors,
/// parsing errors, and configuration issues.
///
/// # Error Recovery Strategy
///
/// - **Walker errors** ([`ScanError::Walk`]): Fatal - propagate immediately
/// - **File read errors** ([`ScanError::Read`]): Log warning, skip file, continue scan
/// - **Parse errors** ([`ScanError::Parse`]): Log warning, skip file, continue scan
///
/// # Examples
///
/// ```
/// use ch_scanner::ScanError;
/// use camino::Utf8PathBuf;
///
/// fn handle_error(err: ScanError) {
///     match err {
///         ScanError::Walk(e) => eprintln!("Walk error: {e}"),
///         ScanError::Read { path, .. } => eprintln!("Read error: {path}"),
///         ScanError::Parse { path, .. } => eprintln!("Parse error: {path}"),
///         ScanError::Config(msg) => eprintln!("Config error: {msg}"),
///         ScanError::NonUtf8Path(p) => eprintln!("Invalid path: {}", p.display()),
///     }
/// }
/// ```
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// Failed to walk a directory.
    ///
    /// This is typically a fatal error that prevents scanning from continuing.
    #[error("failed to walk directory: {0}")]
    Walk(#[from] ignore::Error),

    /// Failed to read a file.
    ///
    /// Contains the path that failed and the underlying I/O error.
    /// Scanning can continue by skipping this file.
    #[error("failed to read file {path}: {source}")]
    Read {
        /// The path of the file that couldn't be read.
        path: Utf8PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse a TypeScript file.
    ///
    /// Contains the path that failed and the underlying parse error.
    /// Scanning can continue by skipping this file.
    #[error("failed to parse file {path}: {source}")]
    Parse {
        /// The path of the file that couldn't be parsed.
        path: Utf8PathBuf,
        /// The underlying parse error.
        #[source]
        source: ch_ts_parser::ParseError,
    },

    /// Invalid scanner configuration.
    ///
    /// Indicates that the scanner was configured with invalid parameters.
    #[error("invalid configuration: {0}")]
    Config(String),

    /// A path is not valid UTF-8.
    ///
    /// This crate uses UTF-8 paths throughout. If a non-UTF-8 path is
    /// encountered, it cannot be processed.
    #[error("path is not valid UTF-8: {}", _0.display())]
    NonUtf8Path(std::path::PathBuf),
}

impl ScanError {
    /// Creates a new [`ScanError::Read`] error.
    #[inline]
    pub fn read(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }

    /// Creates a new [`ScanError::Parse`] error.
    #[inline]
    pub fn parse(path: impl Into<Utf8PathBuf>, source: ch_ts_parser::ParseError) -> Self {
        Self::Parse {
            path: path.into(),
            source,
        }
    }

    /// Creates a new [`ScanError::Config`] error.
    #[inline]
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// Returns `true` if this error is recoverable (scanning can continue).
    ///
    /// Recoverable errors are file-specific issues that don't prevent
    /// scanning other files.
    #[inline]
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::Read { .. } | Self::Parse { .. })
    }

    /// Returns `true` if this error is fatal (scanning should stop).
    #[inline]
    #[must_use]
    pub const fn is_fatal(&self) -> bool {
        !self.is_recoverable()
    }

    /// Returns the file path associated with this error, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Utf8PathBuf> {
        match self {
            Self::Read { path, .. } | Self::Parse { path, .. } => Some(path),
            Self::Walk(_) | Self::Config(_) | Self::NonUtf8Path(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_scan_error_read() {
        let err = ScanError::read("src/foo.ts", io::Error::new(io::ErrorKind::NotFound, "not found"));
        assert!(err.is_recoverable());
        assert!(!err.is_fatal());
        assert_eq!(err.path().map(|p| p.as_str()), Some("src/foo.ts"));
        assert!(err.to_string().contains("src/foo.ts"));
    }

    #[test]
    fn test_scan_error_parse() {
        let err = ScanError::parse("src/bar.ts", ch_ts_parser::ParseError::Parse);
        assert!(err.is_recoverable());
        assert!(!err.is_fatal());
        assert_eq!(err.path().map(|p| p.as_str()), Some("src/bar.ts"));
        assert!(err.to_string().contains("src/bar.ts"));
    }

    #[test]
    fn test_scan_error_config() {
        let err = ScanError::config("invalid root path");
        assert!(!err.is_recoverable());
        assert!(err.is_fatal());
        assert!(err.path().is_none());
        assert!(err.to_string().contains("invalid root path"));
    }

    #[test]
    fn test_scan_error_non_utf8() {
        use std::path::PathBuf;
        let err = ScanError::NonUtf8Path(PathBuf::from("test"));
        assert!(!err.is_recoverable());
        assert!(err.is_fatal());
        assert!(err.path().is_none());
    }

    #[test]
    fn test_scan_error_display() {
        let err = ScanError::Config("test error".to_owned());
        assert_eq!(err.to_string(), "invalid configuration: test error");
    }
}
