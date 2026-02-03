//! Model source detection from import paths.
//!
//! This module provides functions to detect whether an import path references
//! model-specific paths in the legacy `shared/` or new `shared_2023/` directories.
//!
//! # Model-Specific Paths
//!
//! Only imports from the following paths are considered model imports:
//!
//! **Legacy (`shared/`):**
//! - `shared/interfaces` or `shared/interfaces.ts` - interfaces file
//! - `shared/models/` - model files and codegen subdirectory
//!
//! **Modern (`shared_2023/`):**
//! - `shared_2023/interfaces` or `shared_2023/interfaces.codegen` - codegen interfaces
//! - `shared_2023/models/` - model files and codegen subdirectory
//!
//! Other imports from shared directories (e.g., `shared/utils/`, `shared/services/`)
//! are **not** considered model imports and will return `None`.

use ch_core::ModelSource;

/// Detects the [`ModelSource`] from an import path.
///
/// Analyzes the import path to determine if it references models from the
/// legacy `shared/` directory or the new `shared_2023/` directory.
///
/// **Important:** Only imports from model-specific paths (`models/` or `interfaces`)
/// are detected. Other imports from shared directories (e.g., `shared/utils/`,
/// `shared/services/`) return `None`.
///
/// # Arguments
///
/// * `import_path` - The raw import path, may include quotes
///
/// # Returns
///
/// - `Some(ModelSource::Shared2023)` if the path references `shared_2023/models` or `shared_2023/interfaces`
/// - `Some(ModelSource::SharedLegacy)` if the path references `shared/models` or `shared/interfaces`
/// - `None` if the path doesn't reference model-specific paths in either shared directory
///
/// # Examples
///
/// ```
/// use ch_ts_parser::detect_model_source;
/// use ch_core::ModelSource;
///
/// // Legacy shared model imports
/// assert_eq!(
///     detect_model_source("'../shared/models/foo'"),
///     Some(ModelSource::SharedLegacy)
/// );
/// assert_eq!(
///     detect_model_source("'../shared/interfaces'"),
///     Some(ModelSource::SharedLegacy)
/// );
///
/// // New shared_2023 model imports
/// assert_eq!(
///     detect_model_source("'../shared_2023/models/foo'"),
///     Some(ModelSource::Shared2023)
/// );
///
/// // Non-model shared imports return None
/// assert_eq!(detect_model_source("'../shared/utils/helper'"), None);
/// assert_eq!(detect_model_source("'../shared/services/api'"), None);
///
/// // Non-shared imports return None
/// assert_eq!(detect_model_source("'@angular/core'"), None);
/// ```
#[inline]
pub fn detect_model_source(import_path: &str) -> Option<ModelSource> {
    let path = strip_quotes(import_path);

    // Check shared_2023 first (more specific match)
    if is_shared_2023_model_import(path) {
        return Some(ModelSource::Shared2023);
    }

    // Then check legacy shared
    if is_shared_legacy_model_import(path) {
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

/// Checks if the path references model-specific paths in `shared_2023/`.
///
/// Only matches:
/// - `shared_2023/models` or `/shared_2023/models` (model files)
/// - `shared_2023/interfaces` or `/shared_2023/interfaces` (interfaces file)
///
/// Does NOT match other `shared_2023/` subdirectories like `utils/`, `services/`, etc.
#[inline]
fn is_shared_2023_model_import(path: &str) -> bool {
    // Match shared_2023/models or shared_2023/interfaces patterns
    path.contains("/shared_2023/models")
        || path.starts_with("shared_2023/models")
        || path.contains("/shared_2023/interfaces")
        || path.starts_with("shared_2023/interfaces")
        // Handle barrel imports like '../shared_2023/models' (no trailing path)
        || path.ends_with("/shared_2023/models")
        || path.ends_with("/shared_2023/interfaces")
        || path == "shared_2023/models"
        || path == "shared_2023/interfaces"
}

/// Checks if the path references model-specific paths in the legacy `shared/` directory.
///
/// Only matches:
/// - `shared/models` or `/shared/models` (model files)
/// - `shared/interfaces` or `/shared/interfaces` (interfaces file)
///
/// Does NOT match:
/// - Paths containing `shared_2023` (to avoid false positives)
/// - Other `shared/` subdirectories like `utils/`, `services/`, `components/`, etc.
#[inline]
fn is_shared_legacy_model_import(path: &str) -> bool {
    // Exclude shared_2023 first to avoid false positives
    if path.contains("shared_2023") {
        return false;
    }

    // Match shared/models or shared/interfaces patterns
    path.contains("/shared/models")
        || path.starts_with("shared/models")
        || path.contains("/shared/interfaces")
        || path.starts_with("shared/interfaces")
        // Handle barrel imports like '../shared/models' (no trailing path)
        || path.ends_with("/shared/models")
        || path.ends_with("/shared/interfaces")
        || path == "shared/models"
        || path == "shared/interfaces"
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
    fn test_detect_legacy_shared_models() {
        // Relative paths to models
        assert_eq!(
            detect_model_source("'../shared/models/foo'"),
            Some(ModelSource::SharedLegacy)
        );
        assert_eq!(
            detect_model_source("\"../../shared/models/foo\""),
            Some(ModelSource::SharedLegacy)
        );

        // Direct paths to models
        assert_eq!(
            detect_model_source("'shared/models/foo'"),
            Some(ModelSource::SharedLegacy)
        );

        // With extension
        assert_eq!(
            detect_model_source("'../shared/models/foo.ts'"),
            Some(ModelSource::SharedLegacy)
        );

        // Codegen subdirectory
        assert_eq!(
            detect_model_source("'../shared/models/codegen/foo.codegen'"),
            Some(ModelSource::SharedLegacy)
        );

        // Barrel import to models directory
        assert_eq!(
            detect_model_source("'../shared/models'"),
            Some(ModelSource::SharedLegacy)
        );
    }

    #[test]
    fn test_detect_legacy_shared_interfaces() {
        // Interfaces file
        assert_eq!(
            detect_model_source("'../shared/interfaces'"),
            Some(ModelSource::SharedLegacy)
        );

        // Interfaces with extension
        assert_eq!(
            detect_model_source("'../shared/interfaces.ts'"),
            Some(ModelSource::SharedLegacy)
        );

        // Direct path to interfaces
        assert_eq!(
            detect_model_source("'shared/interfaces'"),
            Some(ModelSource::SharedLegacy)
        );
    }

    #[test]
    fn test_detect_shared_2023_models() {
        // Relative paths to models
        assert_eq!(
            detect_model_source("'../shared_2023/models/foo'"),
            Some(ModelSource::Shared2023)
        );
        assert_eq!(
            detect_model_source("\"../../shared_2023/models/foo\""),
            Some(ModelSource::Shared2023)
        );

        // Direct paths to models
        assert_eq!(
            detect_model_source("'shared_2023/models/foo'"),
            Some(ModelSource::Shared2023)
        );

        // With extension
        assert_eq!(
            detect_model_source("'../shared_2023/models/foo.ts'"),
            Some(ModelSource::Shared2023)
        );

        // Codegen subdirectory
        assert_eq!(
            detect_model_source("'../shared_2023/models/codegen/foo.codegen'"),
            Some(ModelSource::Shared2023)
        );

        // Barrel import to models directory
        assert_eq!(
            detect_model_source("'../shared_2023/models'"),
            Some(ModelSource::Shared2023)
        );
    }

    #[test]
    fn test_detect_shared_2023_interfaces() {
        // Interfaces codegen file
        assert_eq!(
            detect_model_source("'../shared_2023/interfaces.codegen'"),
            Some(ModelSource::Shared2023)
        );

        // Direct path to interfaces
        assert_eq!(
            detect_model_source("'shared_2023/interfaces'"),
            Some(ModelSource::Shared2023)
        );

        // Interfaces with extension
        assert_eq!(
            detect_model_source("'../shared_2023/interfaces.codegen.ts'"),
            Some(ModelSource::Shared2023)
        );
    }

    #[test]
    fn test_detect_non_model_shared_imports() {
        // These imports are from shared/ but NOT from models/ or interfaces
        // They should return None because they're not model imports
        assert_eq!(detect_model_source("'../shared/utils/helper'"), None);
        assert_eq!(detect_model_source("'../shared/services/api'"), None);
        assert_eq!(detect_model_source("'../shared/constants'"), None);
        assert_eq!(detect_model_source("'shared/components/foo'"), None);
        assert_eq!(detect_model_source("'../shared/pipes/date'"), None);
        assert_eq!(detect_model_source("'../shared/directives/click'"), None);
        assert_eq!(detect_model_source("'../shared/guards/auth'"), None);

        // Same for shared_2023
        assert_eq!(detect_model_source("'../shared_2023/utils/helper'"), None);
        assert_eq!(detect_model_source("'../shared_2023/services/api'"), None);
        assert_eq!(detect_model_source("'../shared_2023/constants'"), None);
        assert_eq!(detect_model_source("'shared_2023/components/foo'"), None);
    }

    #[test]
    fn test_detect_non_shared_imports() {
        // External packages
        assert_eq!(detect_model_source("'@angular/core'"), None);
        assert_eq!(detect_model_source("'rxjs'"), None);
        assert_eq!(detect_model_source("'rxjs/operators'"), None);
        assert_eq!(detect_model_source("'lodash'"), None);

        // Local files not in shared
        assert_eq!(detect_model_source("'./local-file'"), None);
        assert_eq!(detect_model_source("'../components/foo'"), None);
        assert_eq!(detect_model_source("'../../app/services/data'"), None);
    }

    #[test]
    fn test_shared_2023_takes_precedence() {
        // When a path has both shared_2023/models and could theoretically match shared/models,
        // shared_2023 should win because we check it first
        assert_eq!(
            detect_model_source("'../shared_2023/models/shared/models/foo'"),
            Some(ModelSource::Shared2023)
        );

        // Edge case: paths like shared_2023/shared/models don't match either pattern
        // because they're not valid model paths (no shared_2023/models or shared/interfaces)
        assert_eq!(
            detect_model_source("'shared_2023/shared/models/foo'"),
            None
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

    #[test]
    fn test_is_shared_2023_model_import() {
        // Should match
        assert!(is_shared_2023_model_import("../shared_2023/models/foo"));
        assert!(is_shared_2023_model_import("shared_2023/models/foo"));
        assert!(is_shared_2023_model_import("../shared_2023/interfaces"));
        assert!(is_shared_2023_model_import("shared_2023/interfaces"));
        assert!(is_shared_2023_model_import("../shared_2023/models"));
        assert!(is_shared_2023_model_import("shared_2023/models"));

        // Should NOT match
        assert!(!is_shared_2023_model_import("../shared_2023/utils/foo"));
        assert!(!is_shared_2023_model_import("shared_2023/services/api"));
        assert!(!is_shared_2023_model_import("../shared_2023/constants"));
    }

    #[test]
    fn test_is_shared_legacy_model_import() {
        // Should match
        assert!(is_shared_legacy_model_import("../shared/models/foo"));
        assert!(is_shared_legacy_model_import("shared/models/foo"));
        assert!(is_shared_legacy_model_import("../shared/interfaces"));
        assert!(is_shared_legacy_model_import("shared/interfaces"));
        assert!(is_shared_legacy_model_import("../shared/models"));
        assert!(is_shared_legacy_model_import("shared/models"));

        // Should NOT match (non-model paths)
        assert!(!is_shared_legacy_model_import("../shared/utils/foo"));
        assert!(!is_shared_legacy_model_import("shared/services/api"));
        assert!(!is_shared_legacy_model_import("../shared/constants"));

        // Should NOT match (shared_2023 paths)
        assert!(!is_shared_legacy_model_import("../shared_2023/models/foo"));
        assert!(!is_shared_legacy_model_import("shared_2023/interfaces"));
    }
}
