//! Error types for the ch-core crate.
//!
//! This module provides the [`ConfigError`] type for configuration-related errors
//! that can occur across the workspace.

use camino::Utf8PathBuf;

/// Errors that can occur during configuration loading and validation.
///
/// This error type covers all configuration-related failures including
/// path validation, missing directories, and parsing errors.
///
/// # Examples
///
/// ```
/// use ch_core::ConfigError;
/// use camino::Utf8PathBuf;
///
/// let error = ConfigError::MissingDirectory(Utf8PathBuf::from("/some/path"));
/// assert!(error.to_string().contains("/some/path"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The provided path is invalid or malformed.
    #[error("invalid path '{path}': {reason}")]
    InvalidPath {
        /// The invalid path.
        path: Utf8PathBuf,
        /// Explanation of why the path is invalid.
        reason: String,
    },

    /// A required directory does not exist.
    #[error("missing required directory: {0}")]
    MissingDirectory(Utf8PathBuf),

    /// A configuration option has an invalid value.
    #[error("invalid configuration option '{option}': {reason}")]
    InvalidOption {
        /// The name of the invalid option.
        option: String,
        /// Explanation of why the option is invalid.
        reason: String,
    },

    /// An I/O error occurred while reading configuration.
    #[error("failed to read configuration: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse the configuration file.
    #[error("failed to parse configuration: {0}")]
    Parse(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_path_display() {
        let error = ConfigError::InvalidPath {
            path: Utf8PathBuf::from("/invalid/path"),
            reason: "path contains invalid characters".to_owned(),
        };
        let msg = error.to_string();
        assert!(msg.contains("/invalid/path"));
        assert!(msg.contains("invalid characters"));
    }

    #[test]
    fn test_missing_directory_display() {
        let error = ConfigError::MissingDirectory(Utf8PathBuf::from("/missing/dir"));
        assert!(error.to_string().contains("/missing/dir"));
    }

    #[test]
    fn test_invalid_option_display() {
        let error = ConfigError::InvalidOption {
            option: "max_jobs".to_owned(),
            reason: "must be positive".to_owned(),
        };
        let msg = error.to_string();
        assert!(msg.contains("max_jobs"));
        assert!(msg.contains("must be positive"));
    }
}
