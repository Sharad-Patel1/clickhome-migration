//! TUI-specific error types.
//!
//! This module provides the [`TuiError`] type for handling errors
//! that can occur during TUI operations.

use thiserror::Error;

/// Errors that can occur in the TUI.
///
/// This enum captures all error conditions specific to the terminal
/// user interface, including terminal initialization failures,
/// event handling issues, and integration errors with other crates.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TuiError {
    /// Terminal initialization or operation failed.
    #[error("terminal error: {0}")]
    Terminal(#[from] std::io::Error),

    /// Event channel was closed unexpectedly.
    #[error("event channel closed unexpectedly")]
    ChannelClosed,

    /// Scanner operation failed.
    #[error("scanner error: {0}")]
    Scanner(#[from] ch_scanner::ScanError),

    /// File watcher operation failed.
    #[error("watcher error: {0}")]
    Watcher(#[from] ch_watcher::WatchError),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),
}

impl TuiError {
    /// Creates a new configuration error.
    #[must_use]
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// Returns `true` if this error is recoverable.
    ///
    /// Non-recoverable errors typically require restarting the TUI.
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::Scanner(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error() {
        let err = TuiError::config("invalid tick rate");
        assert!(matches!(err, TuiError::Config(_)));
    }

    #[test]
    fn test_error_display() {
        let err = TuiError::ChannelClosed;
        assert_eq!(err.to_string(), "event channel closed unexpectedly");
    }

    #[test]
    fn test_is_recoverable() {
        assert!(!TuiError::ChannelClosed.is_recoverable());
        assert!(!TuiError::config("test").is_recoverable());
    }
}
