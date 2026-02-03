//! Model source detection from import paths.
//!
//! This module provides functions to detect whether an import path references
//! the legacy `shared/` directory or the new `shared_2023/` directory.

use ch_core::ModelSource;

/// Detects the [`ModelSource`] from an import path.
///
/// Analyzes the import path to determine if it references models from the
/// legacy `shared/` directory or the new `shared_2023/` directory.
///
/// # Arguments
///
/// * `import_path` - The raw import path, may include quotes
///
/// # Returns
///
/// - `Some(ModelSource::Shared2023)` if the path contains `shared_2023`
/// - `Some(ModelSource::SharedLegacy)` if the path contains `shared` (but not `shared_2023`)
/// - `None` if the path doesn't reference either shared directory
///
/// # Examples
///
/// ```
/// use ch_ts_parser::detect_model_source;
/// use ch_core::ModelSource;
///
/// // Legacy shared imports
/// assert_eq!(
///     detect_model_source("'../shared/models/foo'"),
///     Some(ModelSource::SharedLegacy)
/// );
///
/// // New shared_2023 imports
/// assert_eq!(
///     detect_model_source("'../shared_2023/models/foo'"),
///     Some(ModelSource::Shared2023)
/// );
///
/// // Non-shared imports
/// assert_eq!(detect_model_source("'@angular/core'"), None);
/// ```
#[inline]
pub fn detect_model_source(import_path: &str) -> Option<ModelSource> {
    let path = strip_quotes(import_path);

    // Check shared_2023 first (more specific match)
    if contains_shared_2023(path) {
        return Some(ModelSource::Shared2023);
    }

    // Then check legacy shared
    if contains_shared_legacy(path) {
        return Some(ModelSource::SharedLegacy);
    }

    None
}

/// Strips leading and trailing quotes from a string literal.
///
/// Handles both single quotes (`'`) and double quotes (`"`).
#[inline]
fn strip_quotes(s: &str) -> &str {
    s.trim_matches(|c| c == '"' || c == '\'')
}

/// Checks if the path references the `shared_2023` directory.
///
/// Matches patterns like:
/// - `/shared_2023/`
/// - `shared_2023/`
/// - Path ending in `shared_2023`
#[inline]
fn contains_shared_2023(path: &str) -> bool {
    // Match /shared_2023/ or shared_2023/ at start
    path.contains("/shared_2023/")
        || path.starts_with("shared_2023/")
        || path.contains("/shared_2023")
        || path == "shared_2023"
}

/// Checks if the path references the legacy `shared` directory.
///
/// Matches patterns like:
/// - `/shared/`
/// - `shared/`
/// - Path ending in `shared`
///
/// Excludes paths that contain `shared_2023` to avoid false positives.
#[inline]
fn contains_shared_legacy(path: &str) -> bool {
    // Exclude shared_2023 first
    if path.contains("shared_2023") {
        return false;
    }

    // Match /shared/ or shared/ at start
    path.contains("/shared/")
        || path.starts_with("shared/")
        || path.ends_with("/shared")
        || path == "shared"
}

/// Extracts the model name from an import path.
///
/// Given a path like `../shared/models/active-contract`, returns `active-contract`.
///
/// # Arguments
///
/// * `import_path` - The raw import path
///
/// # Returns
///
/// The model name (last path segment), or `None` if the path is empty.
///
/// # Examples
///
/// ```
/// use ch_ts_parser::source::extract_model_name;
///
/// assert_eq!(
///     extract_model_name("'../shared/models/active-contract'"),
///     Some("active-contract")
/// );
/// assert_eq!(
///     extract_model_name("'../shared/interfaces'"),
///     Some("interfaces")
/// );
/// ```
pub fn extract_model_name(import_path: &str) -> Option<&str> {
    let path = strip_quotes(import_path);

    // Remove .ts extension if present
    let path = path.strip_suffix(".ts").unwrap_or(path);

    // Get the last segment
    path.rsplit('/').next().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_legacy_shared() {
        // Relative paths
        assert_eq!(
            detect_model_source("'../shared/models/foo'"),
            Some(ModelSource::SharedLegacy)
        );
        assert_eq!(
            detect_model_source("\"../../shared/models/foo\""),
            Some(ModelSource::SharedLegacy)
        );

        // Direct paths
        assert_eq!(
            detect_model_source("'shared/models/foo'"),
            Some(ModelSource::SharedLegacy)
        );

        // With extension
        assert_eq!(
            detect_model_source("'../shared/models/foo.ts'"),
            Some(ModelSource::SharedLegacy)
        );

        // Interfaces file
        assert_eq!(
            detect_model_source("'../shared/interfaces'"),
            Some(ModelSource::SharedLegacy)
        );
    }

    #[test]
    fn test_detect_shared_2023() {
        // Relative paths
        assert_eq!(
            detect_model_source("'../shared_2023/models/foo'"),
            Some(ModelSource::Shared2023)
        );
        assert_eq!(
            detect_model_source("\"../../shared_2023/models/foo\""),
            Some(ModelSource::Shared2023)
        );

        // Direct paths
        assert_eq!(
            detect_model_source("'shared_2023/models/foo'"),
            Some(ModelSource::Shared2023)
        );

        // With extension
        assert_eq!(
            detect_model_source("'../shared_2023/models/foo.ts'"),
            Some(ModelSource::Shared2023)
        );

        // Interfaces codegen file
        assert_eq!(
            detect_model_source("'../shared_2023/interfaces.codegen'"),
            Some(ModelSource::Shared2023)
        );
    }

    #[test]
    fn test_detect_non_shared() {
        assert_eq!(detect_model_source("'@angular/core'"), None);
        assert_eq!(detect_model_source("'rxjs'"), None);
        assert_eq!(detect_model_source("'./local-file'"), None);
        assert_eq!(detect_model_source("'../components/foo'"), None);
    }

    #[test]
    fn test_shared_2023_takes_precedence() {
        // If somehow a path contained both (edge case), shared_2023 should win
        assert_eq!(
            detect_model_source("'shared_2023/shared/models/foo'"),
            Some(ModelSource::Shared2023)
        );
    }

    #[test]
    fn test_strip_quotes() {
        assert_eq!(strip_quotes("'foo'"), "foo");
        assert_eq!(strip_quotes("\"foo\""), "foo");
        assert_eq!(strip_quotes("foo"), "foo");
        assert_eq!(strip_quotes("''"), "");
    }

    #[test]
    fn test_extract_model_name() {
        assert_eq!(
            extract_model_name("'../shared/models/active-contract'"),
            Some("active-contract")
        );
        assert_eq!(
            extract_model_name("'../shared/interfaces'"),
            Some("interfaces")
        );
        assert_eq!(
            extract_model_name("'../shared/models/foo.ts'"),
            Some("foo")
        );
        assert_eq!(extract_model_name("''"), None);
    }
}
