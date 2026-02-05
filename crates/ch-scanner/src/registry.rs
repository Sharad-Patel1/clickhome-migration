//! Model registry builder for scanning and indexing model definitions.
//!
//! This module provides [`RegistryBuilder`] which scans the shared model
//! directories to build a [`ModelRegistry`] containing all known model exports.
//!
//! # Usage
//!
//! ```ignore
//! use ch_scanner::RegistryBuilder;
//! use camino::Utf8Path;
//!
//! let builder = RegistryBuilder::new(
//!     Utf8Path::new("./WebApp.Desktop/src/shared"),
//!     Utf8Path::new("./WebApp.Desktop/src/shared_2023"),
//! );
//!
//! let registry = builder.build()?;
//!
//! // Check if an import name is a known model export
//! if registry.is_legacy_export("ActiveContractCodeGen") {
//!     println!("Found legacy model export");
//! }
//! ```
//!
//! # Scanned Files
//!
//! The builder scans the following locations:
//!
//! - `shared/interfaces.ts` - Legacy interface definitions
//! - `shared/interfaces.codegen.ts` - Legacy codegen interfaces
//! - `shared/models/*.ts` - Legacy model files
//! - `shared_2023/interfaces.ts` - Modern interface definitions
//! - `shared_2023/interfaces.codegen.ts` - Modern codegen interfaces
//! - `shared_2023/models/*.ts` - Modern model files

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use ch_core::{ModelDefinition, ModelRegistry, ModelSource};
use ch_ts_parser::{extract_exports, get_typescript_export_query, kebab_to_pascal, ExportInfo};
use rayon::prelude::*;
use smallvec::SmallVec;
use tracing::{debug, info, warn};

use crate::error::ScanError;

/// Builder for constructing a [`ModelRegistry`] from the shared directories.
///
/// Scans model definition files and interface files to build a comprehensive
/// registry of all known model exports.
///
/// # Example
///
/// ```ignore
/// use ch_scanner::RegistryBuilder;
/// use camino::Utf8Path;
///
/// let builder = RegistryBuilder::new(
///     Utf8Path::new("./src/shared"),
///     Utf8Path::new("./src/shared_2023"),
/// );
///
/// let registry = builder.build()?;
/// println!("Found {} legacy models", registry.legacy_model_count());
/// println!("Found {} modern models", registry.modern_model_count());
/// ```
#[derive(Debug, Clone)]
pub struct RegistryBuilder {
    /// Path to the legacy shared directory.
    shared_path: Utf8PathBuf,

    /// Path to the modern `shared_2023` directory.
    shared_2023_path: Utf8PathBuf,
}

impl RegistryBuilder {
    /// Creates a new registry builder with the given shared directory paths.
    ///
    /// # Arguments
    ///
    /// * `shared_path` - Path to the legacy `shared/` directory
    /// * `shared_2023_path` - Path to the modern `shared_2023/` directory
    #[must_use]
    pub fn new(shared_path: &Utf8Path, shared_2023_path: &Utf8Path) -> Self {
        Self {
            shared_path: shared_path.to_owned(),
            shared_2023_path: shared_2023_path.to_owned(),
        }
    }

    /// Creates a new registry builder, inferring paths from a root directory.
    ///
    /// Assumes the standard structure where `shared/` and `shared_2023/` are
    /// subdirectories of the given root.
    ///
    /// # Arguments
    ///
    /// * `root` - Root directory containing shared subdirectories
    #[must_use]
    pub fn from_root(root: &Utf8Path) -> Self {
        Self {
            shared_path: root.join("shared"),
            shared_2023_path: root.join("shared_2023"),
        }
    }

    /// Builds the model registry by scanning all model definition files.
    ///
    /// This method:
    /// 1. Parses interface files from both directories
    /// 2. Scans model directories for individual model files
    /// 3. Extracts exports from each file
    /// 4. Builds the registry with all found exports
    ///
    /// # Returns
    ///
    /// A populated [`ModelRegistry`] on success.
    ///
    /// # Errors
    ///
    /// Returns [`ScanError`] if:
    /// - The export query fails to compile
    /// - Critical directories cannot be read (non-critical failures are logged but ignored)
    pub fn build(&self) -> Result<ModelRegistry, ScanError> {
        info!(
            shared = %self.shared_path,
            shared_2023 = %self.shared_2023_path,
            "Building model registry"
        );

        let mut registry = ModelRegistry::new();

        // Parse legacy interfaces and models
        Self::parse_interfaces_file(
            &self.shared_path.join("interfaces.ts"),
            ModelSource::SharedLegacy,
            &mut registry,
        );
        Self::parse_interfaces_file(
            &self.shared_path.join("interfaces.codegen.ts"),
            ModelSource::SharedLegacy,
            &mut registry,
        );
        Self::scan_model_directory(
            &self.shared_path.join("models"),
            ModelSource::SharedLegacy,
            &mut registry,
        );

        // Parse modern interfaces and models
        Self::parse_interfaces_file(
            &self.shared_2023_path.join("interfaces.ts"),
            ModelSource::Shared2023,
            &mut registry,
        );
        Self::parse_interfaces_file(
            &self.shared_2023_path.join("interfaces.codegen.ts"),
            ModelSource::Shared2023,
            &mut registry,
        );
        Self::scan_model_directory(
            &self.shared_2023_path.join("models"),
            ModelSource::Shared2023,
            &mut registry,
        );

        info!(
            legacy_models = registry.legacy_model_count(),
            modern_models = registry.modern_model_count(),
            legacy_exports = registry.legacy_export_count(),
            modern_exports = registry.modern_export_count(),
            "Model registry built"
        );

        Ok(registry)
    }

    /// Parses an interfaces file and registers all exports.
    ///
    /// Interface files typically contain many interface declarations and
    /// are treated as a single "interfaces" model in the registry.
    fn parse_interfaces_file(path: &Utf8Path, source: ModelSource, registry: &mut ModelRegistry) {
        if !path.exists() {
            debug!(path = %path, "Interfaces file not found, skipping");
            return;
        }

        let contents = match fs::read_to_string(path.as_std_path()) {
            Ok(c) => c,
            Err(e) => {
                warn!(path = %path, error = %e, "Failed to read interfaces file");
                return;
            }
        };

        let exports = match Self::extract_exports_from_source(&contents) {
            Ok(e) => e,
            Err(e) => {
                warn!(path = %path, error = %e, "Failed to parse interfaces file");
                return;
            }
        };

        if exports.is_empty() {
            debug!(path = %path, "No exports found in interfaces file");
            return;
        }

        // Create a model definition for the interfaces file
        let model_name = path.file_stem().unwrap_or("interfaces").to_owned();

        let mut definition = ModelDefinition::new(model_name, source, path);
        for export in &exports {
            definition.add_export(&export.name);
        }

        debug!(
            path = %path,
            export_count = exports.len(),
            "Registered interfaces file"
        );

        registry.register(definition);
    }

    /// Scans a model directory and registers all model files.
    ///
    /// Each `.ts` file in the models directory is treated as a separate model.
    /// The model name is derived from the filename using kebab-to-pascal conversion.
    fn scan_model_directory(dir: &Utf8Path, source: ModelSource, registry: &mut ModelRegistry) {
        if !dir.exists() {
            debug!(dir = %dir, "Models directory not found, skipping");
            return;
        }

        // Collect all TypeScript files in the directory
        let entries: Vec<_> = match fs::read_dir(dir.as_std_path()) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|ext| ext == "ts" || ext == "tsx")
                })
                .collect(),
            Err(e) => {
                warn!(dir = %dir, error = %e, "Failed to read models directory");
                return;
            }
        };

        if entries.is_empty() {
            debug!(dir = %dir, "No TypeScript files found in models directory");
            return;
        }

        // Process files in parallel
        let results: Vec<_> = entries
            .par_iter()
            .filter_map(|entry| {
                let path = entry.path();
                let utf8_path = Utf8PathBuf::try_from(path.clone()).ok()?;
                let contents = fs::read_to_string(&path).ok()?;
                let exports = Self::extract_exports_from_source(&contents).ok()?;

                if exports.is_empty() {
                    return None;
                }

                // Derive model name from filename
                let model_name = utf8_path
                    .file_stem()
                    .map(kebab_to_pascal)
                    .unwrap_or_default();

                if model_name.is_empty() {
                    return None;
                }

                let mut definition = ModelDefinition::new(&model_name, source, &utf8_path);
                for export in &exports {
                    definition.add_export(&export.name);
                }

                Some(definition)
            })
            .collect();

        // Register all found definitions
        for definition in results {
            debug!(
                model = &definition.name,
                exports = definition.exports.len(),
                "Registered model"
            );
            registry.register(definition);
        }
    }

    /// Extracts exports from TypeScript source code.
    fn extract_exports_from_source(source: &str) -> Result<SmallVec<[ExportInfo; 16]>, ScanError> {
        let query = get_typescript_export_query().map_err(|e| ScanError::config(e.to_string()))?;

        // Use ch_ts_parser's TsParser for parsing
        let mut parser =
            ch_ts_parser::TsParser::new().map_err(|e| ScanError::config(e.to_string()))?;

        // Parse to get the tree - we only need the tree for export extraction
        let parse_result = parser
            .parse(source)
            .map_err(|e| ScanError::config(e.to_string()))?;

        Ok(extract_exports(&parse_result.tree, source, query))
    }
}

/// Result of building a model registry.
#[derive(Debug)]
pub struct RegistryBuildResult {
    /// The built registry.
    pub registry: ModelRegistry,

    /// Paths that failed to parse (non-fatal).
    pub parse_errors: Vec<(Utf8PathBuf, String)>,

    /// Paths that failed to read (non-fatal).
    pub read_errors: Vec<(Utf8PathBuf, String)>,
}

impl RegistryBuildResult {
    /// Returns `true` if there were any errors during building.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.parse_errors.is_empty() || !self.read_errors.is_empty()
    }

    /// Returns the total number of errors.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.parse_errors.len() + self.read_errors.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_builder_from_root() {
        let builder = RegistryBuilder::from_root(Utf8Path::new("/app/src"));
        assert_eq!(builder.shared_path, Utf8PathBuf::from("/app/src/shared"));
        assert_eq!(
            builder.shared_2023_path,
            Utf8PathBuf::from("/app/src/shared_2023")
        );
    }

    #[test]
    fn test_registry_builder_new() {
        let builder = RegistryBuilder::new(
            Utf8Path::new("/custom/shared"),
            Utf8Path::new("/custom/shared_2023"),
        );
        assert_eq!(builder.shared_path, Utf8PathBuf::from("/custom/shared"));
        assert_eq!(
            builder.shared_2023_path,
            Utf8PathBuf::from("/custom/shared_2023")
        );
    }

    #[test]
    fn test_extract_exports_from_source() {
        let source = r#"
export class FooCodeGen { }
export interface FooModel { }
export { Bar };
"#;

        let exports = RegistryBuilder::extract_exports_from_source(source).unwrap();
        assert_eq!(exports.len(), 3);

        let names: Vec<_> = exports.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"FooCodeGen"));
        assert!(names.contains(&"FooModel"));
        assert!(names.contains(&"Bar"));
    }

    #[test]
    fn test_registry_build_result() {
        let result = RegistryBuildResult {
            registry: ModelRegistry::new(),
            parse_errors: vec![(Utf8PathBuf::from("foo.ts"), "error".to_owned())],
            read_errors: vec![],
        };

        assert!(result.has_errors());
        assert_eq!(result.error_count(), 1);

        let clean_result = RegistryBuildResult {
            registry: ModelRegistry::new(),
            parse_errors: vec![],
            read_errors: vec![],
        };

        assert!(!clean_result.has_errors());
        assert_eq!(clean_result.error_count(), 0);
    }
}
