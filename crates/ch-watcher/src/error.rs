//! Error types for the ch-watcher crate.
//!
//! This module provides the [`WatchError`] type for errors that can occur
//! during file watching operations.

use camino::Utf8PathBuf;

/// Errors that can occur during file watching operations.
///
/// These errors cover watcher initialization failures, path validation,
/// channel communication issues, and I/O errors.
///
/// # Error Recovery Strategy
///
/// - **Notify errors** ([`WatchError::Notify`]): Fatal - propagate immediately
/// - **Path not found** ([`WatchError::PathNotFound`]): Fatal - path must exist
/// - **Channel closed** ([`WatchError::ChannelClosed`]): Fatal - communication broken
/// - **Non-UTF-8 path** ([`WatchError::NonUtf8Path`]): Recoverable - skip and continue
/// - **I/O errors** ([`WatchError::Io`]): Fatal - propagate immediately
///
/// # Examples
///
/// ```
/// use ch_watcher::WatchError;
/// use camino::Utf8PathBuf;
///
/// fn handle_error(err: WatchError) {
///     match err {
///         WatchError::Notify(e) => eprintln!("Notify error: {e}"),
///         WatchError::PathNotFound(p) => eprintln!("Path not found: {p}"),
///         WatchError::ChannelClosed => eprintln!("Channel closed"),
///         WatchError::NonUtf8Path(p) => eprintln!("Invalid path: {}", p.display()),
///         WatchError::Io(e) => eprintln!("I/O error: {e}"),
///     }
/// }
/// ```
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// Failed to initialize or operate the notify watcher.
    ///
    /// This is typically a fatal error that prevents watching from continuing.
    #[error("notify watcher error: {0}")]
    Notify(#[from] notify::Error),

    /// The specified path does not exist.
    ///
    /// The watcher requires a valid, existing path to watch.
    #[error("path does not exist: {0}")]
    PathNotFound(Utf8PathBuf),

    /// The event channel was closed unexpectedly.
    ///
    /// This indicates a communication failure between the watcher thread
    /// and the async event consumer.
    #[error("event channel closed unexpectedly")]
    ChannelClosed,

    /// A path is not valid UTF-8.
    ///
    /// This crate uses UTF-8 paths throughout. If a non-UTF-8 path is
    /// encountered in a file event, it is logged and skipped.
    #[error("path is not valid UTF-8: {}", _0.display())]
    NonUtf8Path(std::path::PathBuf),

    /// An I/O error occurred.
    ///
    /// General I/O errors during path validation or file operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl WatchError {
    /// Creates a new [`WatchError::PathNotFound`] error.
    #[inline]
    pub fn path_not_found(path: impl Into<Utf8PathBuf>) -> Self {
        Self::PathNotFound(path.into())
    }

    /// Creates a new [`WatchError::NonUtf8Path`] error.
    #[inline]
    pub fn non_utf8_path(path: impl Into<std::path::PathBuf>) -> Self {
        Self::NonUtf8Path(path.into())
    }

    /// Returns `true` if this error is recoverable (watching can continue).
    ///
    /// Recoverable errors are event-specific issues that don't prevent
    /// watching other files. Currently, only non-UTF-8 path errors are
    /// recoverable as they can be skipped.
    #[inline]
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::NonUtf8Path(_))
    }

    /// Returns `true` if this error is fatal (watching should stop).
    #[inline]
    #[must_use]
    pub const fn is_fatal(&self) -> bool {
        !self.is_recoverable()
    }

    /// Returns the file path associated with this error, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Utf8PathBuf> {
        match self {
            Self::PathNotFound(path) => Some(path),
            Self::Notify(_) | Self::ChannelClosed | Self::NonUtf8Path(_) | Self::Io(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::PathBuf;

    #[test]
    fn test_watch_error_path_not_found() {
        let err = WatchError::path_not_found("src/missing");
        assert!(!err.is_recoverable());
        assert!(err.is_fatal());
        assert_eq!(err.path().map(|p| p.as_str()), Some("src/missing"));
        assert!(err.to_string().contains("src/missing"));
    }

    #[test]
    fn test_watch_error_channel_closed() {
        let err = WatchError::ChannelClosed;
        assert!(!err.is_recoverable());
        assert!(err.is_fatal());
        assert!(err.path().is_none());
        assert!(err.to_string().contains("channel closed"));
    }

    #[test]
    fn test_watch_error_non_utf8() {
        let err = WatchError::non_utf8_path(PathBuf::from("test"));
        assert!(err.is_recoverable());
        assert!(!err.is_fatal());
        assert!(err.path().is_none());
        assert!(err.to_string().contains("not valid UTF-8"));
    }

    #[test]
    fn test_watch_error_io() {
        let err = WatchError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "access denied",
        ));
        assert!(!err.is_recoverable());
        assert!(err.is_fatal());
        assert!(err.path().is_none());
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn test_watch_error_display() {
        let err = WatchError::PathNotFound(Utf8PathBuf::from("/some/path"));
        assert_eq!(err.to_string(), "path does not exist: /some/path");
    }
}
