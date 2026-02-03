//! Model-related types for tracking TypeScript model references.
//!
//! This module provides types for representing model artifacts in the `ClickHome`
//! codebase, including their source (legacy vs new) and category (interface,
//! codegen, form, etc.).
//!
//! # Registry Types
//!
//! The [`ModelRegistry`] provides O(1) lookup for validating whether an imported
//! name is an actual model from the shared directories:
//!
//! ```
//! use ch_core::types::model::{ModelRegistry, ModelDefinition, ModelSource};
//! use camino::Utf8PathBuf;
//! use smallvec::smallvec;
//!
//! let mut registry = ModelRegistry::new();
//!
//! // Register a legacy model
//! let definition = ModelDefinition {
//!     name: "ActiveContract".to_owned(),
//!     source: ModelSource::SharedLegacy,
//!     definition_path: Utf8PathBuf::from("shared/models/active-contract.ts"),
//!     exports: smallvec!["ActiveContract".to_owned(), "ActiveContractCodeGen".to_owned()],
//! };
//! registry.register(definition);
//!
//! // Check if an import name is a known model export
//! assert!(registry.is_legacy_export("ActiveContract"));
//! assert!(!registry.is_modern_export("ActiveContract"));
//! ```

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{FxHashMap, FxHashSet};

/// The source directory of a model.
///
/// Indicates whether a model comes from the legacy `shared/` directory
/// or the new `shared_2023/` directory.
///
/// # Examples
///
/// ```
/// use ch_core::ModelSource;
///
/// let source = ModelSource::SharedLegacy;
/// assert!(source.is_legacy());
///
/// let source = ModelSource::Shared2023;
/// assert!(!source.is_legacy());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelSource {
    /// Model from the legacy `shared/` directory.
    SharedLegacy,

    /// Model from the new `shared_2023/` directory.
    Shared2023,
}

impl ModelSource {
    /// Returns `true` if this is a legacy model source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::ModelSource;
    ///
    /// assert!(ModelSource::SharedLegacy.is_legacy());
    /// assert!(!ModelSource::Shared2023.is_legacy());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_legacy(self) -> bool {
        matches!(self, Self::SharedLegacy)
    }

    /// Returns the directory name for this model source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::ModelSource;
    ///
    /// assert_eq!(ModelSource::SharedLegacy.dir_name(), "shared");
    /// assert_eq!(ModelSource::Shared2023.dir_name(), "shared_2023");
    /// ```
    #[inline]
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::SharedLegacy => "shared",
            Self::Shared2023 => "shared_2023",
        }
    }
}

/// The category of a model artifact.
///
/// `ClickHome` models consist of multiple related artifacts: interfaces,
/// base codegen classes, API models, forms, and form arrays.
///
/// # Naming Conventions
///
/// - Interface: `{ModelName}Model` (in `interfaces.ts`)
/// - Model: Main model class extending `CodeGen`
/// - `CodeGen`: `{ModelName}CodeGen` (extends `BaseModel`)
/// - `CodeGenForApi`: `{ModelName}CodeGenForApi` (extends `BaseForApiModel`)
/// - `CodeGenForm`: `{ModelName}CodeGenForm` (extends `BaseModelForm`)
/// - `CodeGenFormArray`: `{ModelName}CodeGenFormArray` (extends `BaseModelFormArray`)
///
/// # Examples
///
/// ```
/// use ch_core::ModelCategory;
///
/// let category = ModelCategory::Interface;
/// assert_eq!(category.suffix(), "Model");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelCategory {
    /// Interface type from `interfaces.ts` (naming: `{ModelName}Model`).
    Interface,

    /// Main model class that extends the `CodeGen` class.
    Model,

    /// Base codegen class (naming: `{ModelName}CodeGen`).
    CodeGen,

    /// API model class (naming: `{ModelName}CodeGenForApi`).
    CodeGenForApi,

    /// Form model class (naming: `{ModelName}CodeGenForm`).
    CodeGenForm,

    /// Form array class (naming: `{ModelName}CodeGenFormArray`).
    CodeGenFormArray,
}

impl ModelCategory {
    /// Returns the typical suffix for this category.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::ModelCategory;
    ///
    /// assert_eq!(ModelCategory::Interface.suffix(), "Model");
    /// assert_eq!(ModelCategory::CodeGen.suffix(), "CodeGen");
    /// ```
    #[inline]
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Interface => "Model",
            Self::Model => "",
            Self::CodeGen => "CodeGen",
            Self::CodeGenForApi => "CodeGenForApi",
            Self::CodeGenForm => "CodeGenForm",
            Self::CodeGenFormArray => "CodeGenFormArray",
        }
    }

    /// Returns `true` if this is a codegen-related category.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::ModelCategory;
    ///
    /// assert!(ModelCategory::CodeGen.is_codegen());
    /// assert!(ModelCategory::CodeGenForApi.is_codegen());
    /// assert!(!ModelCategory::Interface.is_codegen());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_codegen(self) -> bool {
        matches!(
            self,
            Self::CodeGen | Self::CodeGenForApi | Self::CodeGenForm | Self::CodeGenFormArray
        )
    }
}

/// A reference to a model in the codebase.
///
/// Represents a specific model artifact, including its name, category,
/// and source directory.
///
/// # Examples
///
/// ```
/// use ch_core::{ModelReference, ModelCategory, ModelSource};
///
/// let model_ref = ModelReference {
///     name: "ActiveContract".to_owned(),
///     category: ModelCategory::Model,
///     source: ModelSource::SharedLegacy,
/// };
///
/// assert_eq!(model_ref.name, "ActiveContract");
/// assert!(model_ref.source.is_legacy());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelReference {
    /// The model name (e.g., `ActiveContract`).
    pub name: String,

    /// The category of this model artifact.
    pub category: ModelCategory,

    /// The source directory (legacy or new).
    pub source: ModelSource,
}

impl ModelReference {
    /// Creates a new model reference.
    ///
    /// # Arguments
    ///
    /// * `name` - The model name
    /// * `category` - The artifact category
    /// * `source` - The source directory
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{ModelReference, ModelCategory, ModelSource};
    ///
    /// let model_ref = ModelReference::new(
    ///     "ActiveContract",
    ///     ModelCategory::CodeGen,
    ///     ModelSource::Shared2023,
    /// );
    ///
    /// assert_eq!(model_ref.name, "ActiveContract");
    /// ```
    #[inline]
    #[must_use]
    pub fn new(name: impl Into<String>, category: ModelCategory, source: ModelSource) -> Self {
        Self {
            name: name.into(),
            category,
            source,
        }
    }

    /// Returns `true` if this reference is from the legacy source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{ModelReference, ModelCategory, ModelSource};
    ///
    /// let legacy = ModelReference::new("Foo", ModelCategory::Model, ModelSource::SharedLegacy);
    /// assert!(legacy.is_legacy());
    ///
    /// let new = ModelReference::new("Foo", ModelCategory::Model, ModelSource::Shared2023);
    /// assert!(!new.is_legacy());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_legacy(&self) -> bool {
        self.source.is_legacy()
    }
}

// =============================================================================
// Registry Types
// =============================================================================

/// The kind of export declaration.
///
/// Used to categorize exports when building the model registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExportKind {
    /// Exported class declaration: `export class Foo { }`
    Class,

    /// Exported interface declaration: `export interface Foo { }`
    Interface,

    /// Named export from export clause: `export { Foo }`
    Named,

    /// Re-export from another module: `export { Foo } from './foo'`
    ReExport,
}

impl ExportKind {
    /// Returns `true` if this is a type declaration (interface).
    #[inline]
    #[must_use]
    pub const fn is_type(self) -> bool {
        matches!(self, Self::Interface)
    }

    /// Returns `true` if this is a class declaration.
    #[inline]
    #[must_use]
    pub const fn is_class(self) -> bool {
        matches!(self, Self::Class)
    }
}

/// A known model definition from the shared directories.
///
/// Represents a model file that exports one or more model-related types
/// (interfaces, classes, codegen classes, etc.).
///
/// # Examples
///
/// ```
/// use ch_core::types::model::{ModelDefinition, ModelSource};
/// use camino::Utf8PathBuf;
/// use smallvec::smallvec;
///
/// let definition = ModelDefinition {
///     name: "ActiveContract".to_owned(),
///     source: ModelSource::SharedLegacy,
///     definition_path: Utf8PathBuf::from("shared/models/active-contract.ts"),
///     exports: smallvec![
///         "ActiveContract".to_owned(),
///         "ActiveContractCodeGen".to_owned(),
///         "ActiveContractModel".to_owned(),
///     ],
/// };
///
/// assert_eq!(definition.name, "ActiveContract");
/// assert!(definition.source.is_legacy());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDefinition {
    /// Base model name (e.g., `ActiveContract`).
    ///
    /// This is typically derived from the filename using kebab-to-pascal
    /// conversion (e.g., `active-contract.ts` → `ActiveContract`).
    pub name: String,

    /// Source directory (legacy `shared/` or modern `shared_2023/`).
    pub source: ModelSource,

    /// File path where this model is defined, relative to the source root.
    pub definition_path: Utf8PathBuf,

    /// All exported class/interface names from this model file.
    ///
    /// Typically includes variations like:
    /// - `{Name}` (main class)
    /// - `{Name}Model` (interface)
    /// - `{Name}CodeGen`
    /// - `{Name}CodeGenForApi`
    /// - `{Name}CodeGenForm`
    /// - `{Name}CodeGenFormArray`
    pub exports: SmallVec<[String; 6]>,
}

impl ModelDefinition {
    /// Creates a new model definition.
    ///
    /// # Arguments
    ///
    /// * `name` - The base model name
    /// * `source` - The source directory
    /// * `definition_path` - Path to the definition file
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        source: ModelSource,
        definition_path: impl Into<Utf8PathBuf>,
    ) -> Self {
        Self {
            name: name.into(),
            source,
            definition_path: definition_path.into(),
            exports: SmallVec::new(),
        }
    }

    /// Adds an export name to this model definition.
    pub fn add_export(&mut self, export_name: impl Into<String>) {
        self.exports.push(export_name.into());
    }

    /// Returns `true` if this model is from the legacy source.
    #[inline]
    #[must_use]
    pub const fn is_legacy(&self) -> bool {
        self.source.is_legacy()
    }
}

/// Registry of all known models from both shared directories.
///
/// Provides O(1) lookup for validating whether an imported name is an actual
/// model export from the `shared/` or `shared_2023/` directories.
///
/// # Thread Safety
///
/// `ModelRegistry` is both `Send` and `Sync`, making it safe to share across
/// threads via `Arc<ModelRegistry>`.
///
/// # Examples
///
/// ```
/// use ch_core::types::model::{ModelRegistry, ModelDefinition, ModelSource};
/// use camino::Utf8PathBuf;
/// use smallvec::smallvec;
///
/// let mut registry = ModelRegistry::new();
///
/// let mut definition = ModelDefinition::new(
///     "ActiveContract",
///     ModelSource::SharedLegacy,
///     "shared/models/active-contract.ts",
/// );
/// definition.add_export("ActiveContract");
/// definition.add_export("ActiveContractCodeGen");
///
/// registry.register(definition);
///
/// assert!(registry.is_legacy_export("ActiveContract"));
/// assert!(registry.is_legacy_export("ActiveContractCodeGen"));
/// assert!(!registry.is_modern_export("ActiveContract"));
/// assert_eq!(registry.legacy_model_count(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    /// Legacy models indexed by base name.
    legacy_models: FxHashMap<String, ModelDefinition>,

    /// Modern models indexed by base name.
    modern_models: FxHashMap<String, ModelDefinition>,

    /// Set of all legacy export names for O(1) lookup.
    legacy_exports: FxHashSet<String>,

    /// Set of all modern export names for O(1) lookup.
    modern_exports: FxHashSet<String>,
}

impl ModelRegistry {
    /// Creates a new empty model registry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::types::model::ModelRegistry;
    ///
    /// let registry = ModelRegistry::new();
    /// assert_eq!(registry.total_model_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry with pre-allocated capacity.
    ///
    /// # Arguments
    ///
    /// * `legacy_capacity` - Expected number of legacy models
    /// * `modern_capacity` - Expected number of modern models
    #[must_use]
    pub fn with_capacity(legacy_capacity: usize, modern_capacity: usize) -> Self {
        Self {
            legacy_models: FxHashMap::with_capacity_and_hasher(
                legacy_capacity,
                crate::FxBuildHasher::default(),
            ),
            modern_models: FxHashMap::with_capacity_and_hasher(
                modern_capacity,
                crate::FxBuildHasher::default(),
            ),
            legacy_exports: FxHashSet::with_capacity_and_hasher(
                legacy_capacity * 6, // Assume ~6 exports per model
                crate::FxBuildHasher::default(),
            ),
            modern_exports: FxHashSet::with_capacity_and_hasher(
                modern_capacity * 6,
                crate::FxBuildHasher::default(),
            ),
        }
    }

    /// Registers a model definition in the registry.
    ///
    /// Adds the model to the appropriate collection (legacy or modern) based
    /// on its source, and indexes all export names for O(1) lookup.
    ///
    /// # Arguments
    ///
    /// * `definition` - The model definition to register
    pub fn register(&mut self, definition: ModelDefinition) {
        match definition.source {
            ModelSource::SharedLegacy => {
                for export in &definition.exports {
                    self.legacy_exports.insert(export.clone());
                }
                self.legacy_models.insert(definition.name.clone(), definition);
            }
            ModelSource::Shared2023 => {
                for export in &definition.exports {
                    self.modern_exports.insert(export.clone());
                }
                self.modern_models.insert(definition.name.clone(), definition);
            }
        }
    }

    /// Registers a legacy model definition.
    ///
    /// This is a convenience method for registering a model with
    /// [`ModelSource::SharedLegacy`].
    pub fn register_legacy(&mut self, mut definition: ModelDefinition) {
        definition.source = ModelSource::SharedLegacy;
        self.register(definition);
    }

    /// Registers a modern model definition.
    ///
    /// This is a convenience method for registering a model with
    /// [`ModelSource::Shared2023`].
    pub fn register_modern(&mut self, mut definition: ModelDefinition) {
        definition.source = ModelSource::Shared2023;
        self.register(definition);
    }

    /// Returns `true` if the given name is a legacy model export.
    ///
    /// # Arguments
    ///
    /// * `name` - The export name to check
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::types::model::{ModelRegistry, ModelDefinition, ModelSource};
    /// use smallvec::smallvec;
    ///
    /// let mut registry = ModelRegistry::new();
    /// let definition = ModelDefinition {
    ///     name: "Foo".to_owned(),
    ///     source: ModelSource::SharedLegacy,
    ///     definition_path: "shared/models/foo.ts".into(),
    ///     exports: smallvec!["Foo".to_owned(), "FooModel".to_owned()],
    /// };
    /// registry.register(definition);
    ///
    /// assert!(registry.is_legacy_export("Foo"));
    /// assert!(registry.is_legacy_export("FooModel"));
    /// assert!(!registry.is_legacy_export("Bar"));
    /// ```
    #[inline]
    #[must_use]
    pub fn is_legacy_export(&self, name: &str) -> bool {
        self.legacy_exports.contains(name)
    }

    /// Returns `true` if the given name is a modern model export.
    ///
    /// # Arguments
    ///
    /// * `name` - The export name to check
    #[inline]
    #[must_use]
    pub fn is_modern_export(&self, name: &str) -> bool {
        self.modern_exports.contains(name)
    }

    /// Returns `true` if the given name is a known model export (legacy or modern).
    ///
    /// # Arguments
    ///
    /// * `name` - The export name to check
    #[inline]
    #[must_use]
    pub fn is_known_export(&self, name: &str) -> bool {
        self.legacy_exports.contains(name) || self.modern_exports.contains(name)
    }

    /// Returns `true` if the given name is a known export from the specified source.
    ///
    /// # Arguments
    ///
    /// * `name` - The export name to check
    /// * `source` - The source to check against
    #[inline]
    #[must_use]
    pub fn is_export_from(&self, name: &str, source: ModelSource) -> bool {
        match source {
            ModelSource::SharedLegacy => self.is_legacy_export(name),
            ModelSource::Shared2023 => self.is_modern_export(name),
        }
    }

    /// Returns the number of legacy models registered.
    #[inline]
    #[must_use]
    pub fn legacy_model_count(&self) -> usize {
        self.legacy_models.len()
    }

    /// Returns the number of modern models registered.
    #[inline]
    #[must_use]
    pub fn modern_model_count(&self) -> usize {
        self.modern_models.len()
    }

    /// Returns the total number of models registered.
    #[inline]
    #[must_use]
    pub fn total_model_count(&self) -> usize {
        self.legacy_models.len() + self.modern_models.len()
    }

    /// Returns the number of legacy export names registered.
    #[inline]
    #[must_use]
    pub fn legacy_export_count(&self) -> usize {
        self.legacy_exports.len()
    }

    /// Returns the number of modern export names registered.
    #[inline]
    #[must_use]
    pub fn modern_export_count(&self) -> usize {
        self.modern_exports.len()
    }

    /// Returns `true` if the registry is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.legacy_models.is_empty() && self.modern_models.is_empty()
    }

    /// Returns an iterator over legacy model definitions.
    pub fn iter_legacy_models(&self) -> impl Iterator<Item = &ModelDefinition> {
        self.legacy_models.values()
    }

    /// Returns an iterator over modern model definitions.
    pub fn iter_modern_models(&self) -> impl Iterator<Item = &ModelDefinition> {
        self.modern_models.values()
    }

    /// Returns an iterator over all model definitions.
    pub fn iter_all_models(&self) -> impl Iterator<Item = &ModelDefinition> {
        self.legacy_models.values().chain(self.modern_models.values())
    }

    /// Returns a legacy model definition by name, if it exists.
    #[must_use]
    pub fn get_legacy_model(&self, name: &str) -> Option<&ModelDefinition> {
        self.legacy_models.get(name)
    }

    /// Returns a modern model definition by name, if it exists.
    #[must_use]
    pub fn get_modern_model(&self, name: &str) -> Option<&ModelDefinition> {
        self.modern_models.get(name)
    }

    /// Clears all registered models from the registry.
    pub fn clear(&mut self) {
        self.legacy_models.clear();
        self.modern_models.clear();
        self.legacy_exports.clear();
        self.modern_exports.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn test_model_source_is_legacy() {
        assert!(ModelSource::SharedLegacy.is_legacy());
        assert!(!ModelSource::Shared2023.is_legacy());
    }

    #[test]
    fn test_model_source_dir_name() {
        assert_eq!(ModelSource::SharedLegacy.dir_name(), "shared");
        assert_eq!(ModelSource::Shared2023.dir_name(), "shared_2023");
    }

    #[test]
    fn test_model_source_serialization() {
        assert_eq!(
            serde_json::to_string(&ModelSource::SharedLegacy).unwrap(),
            r#""shared_legacy""#
        );
        assert_eq!(
            serde_json::to_string(&ModelSource::Shared2023).unwrap(),
            r#""shared2023""#
        );
    }

    #[test]
    fn test_model_category_suffix() {
        assert_eq!(ModelCategory::Interface.suffix(), "Model");
        assert_eq!(ModelCategory::Model.suffix(), "");
        assert_eq!(ModelCategory::CodeGen.suffix(), "CodeGen");
        assert_eq!(ModelCategory::CodeGenForApi.suffix(), "CodeGenForApi");
        assert_eq!(ModelCategory::CodeGenForm.suffix(), "CodeGenForm");
        assert_eq!(ModelCategory::CodeGenFormArray.suffix(), "CodeGenFormArray");
    }

    #[test]
    fn test_model_category_is_codegen() {
        assert!(!ModelCategory::Interface.is_codegen());
        assert!(!ModelCategory::Model.is_codegen());
        assert!(ModelCategory::CodeGen.is_codegen());
        assert!(ModelCategory::CodeGenForApi.is_codegen());
        assert!(ModelCategory::CodeGenForm.is_codegen());
        assert!(ModelCategory::CodeGenFormArray.is_codegen());
    }

    #[test]
    fn test_model_reference_new() {
        let model_ref = ModelReference::new(
            "ActiveContract",
            ModelCategory::CodeGen,
            ModelSource::SharedLegacy,
        );
        assert_eq!(model_ref.name, "ActiveContract");
        assert_eq!(model_ref.category, ModelCategory::CodeGen);
        assert_eq!(model_ref.source, ModelSource::SharedLegacy);
    }

    #[test]
    fn test_model_reference_is_legacy() {
        let legacy =
            ModelReference::new("Foo", ModelCategory::Model, ModelSource::SharedLegacy);
        assert!(legacy.is_legacy());

        let new = ModelReference::new("Foo", ModelCategory::Model, ModelSource::Shared2023);
        assert!(!new.is_legacy());
    }

    #[test]
    fn test_model_reference_serialization() {
        let model_ref = ModelReference::new(
            "ActiveContract",
            ModelCategory::Interface,
            ModelSource::Shared2023,
        );
        let json = serde_json::to_string(&model_ref).unwrap();
        let parsed: ModelReference = serde_json::from_str(&json).unwrap();
        assert_eq!(model_ref, parsed);
    }

    // =========================================================================
    // Registry Tests
    // =========================================================================

    #[test]
    fn test_export_kind_is_type() {
        assert!(ExportKind::Interface.is_type());
        assert!(!ExportKind::Class.is_type());
        assert!(!ExportKind::Named.is_type());
        assert!(!ExportKind::ReExport.is_type());
    }

    #[test]
    fn test_export_kind_is_class() {
        assert!(ExportKind::Class.is_class());
        assert!(!ExportKind::Interface.is_class());
        assert!(!ExportKind::Named.is_class());
        assert!(!ExportKind::ReExport.is_class());
    }

    #[test]
    fn test_model_definition_new() {
        let def = ModelDefinition::new(
            "ActiveContract",
            ModelSource::SharedLegacy,
            "shared/models/active-contract.ts",
        );
        assert_eq!(def.name, "ActiveContract");
        assert!(def.is_legacy());
        assert!(def.exports.is_empty());
    }

    #[test]
    fn test_model_definition_add_export() {
        let mut def = ModelDefinition::new(
            "ActiveContract",
            ModelSource::SharedLegacy,
            "shared/models/active-contract.ts",
        );
        def.add_export("ActiveContract");
        def.add_export("ActiveContractCodeGen");

        assert_eq!(def.exports.len(), 2);
        assert!(def.exports.contains(&"ActiveContract".to_owned()));
        assert!(def.exports.contains(&"ActiveContractCodeGen".to_owned()));
    }

    #[test]
    fn test_model_registry_new() {
        let registry = ModelRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.total_model_count(), 0);
        assert_eq!(registry.legacy_model_count(), 0);
        assert_eq!(registry.modern_model_count(), 0);
    }

    #[test]
    fn test_model_registry_register_legacy() {
        let mut registry = ModelRegistry::new();
        let definition = ModelDefinition {
            name: "Foo".to_owned(),
            source: ModelSource::SharedLegacy,
            definition_path: "shared/models/foo.ts".into(),
            exports: smallvec!["Foo".to_owned(), "FooModel".to_owned(), "FooCodeGen".to_owned()],
        };
        registry.register(definition);

        assert_eq!(registry.legacy_model_count(), 1);
        assert_eq!(registry.modern_model_count(), 0);
        assert_eq!(registry.legacy_export_count(), 3);

        assert!(registry.is_legacy_export("Foo"));
        assert!(registry.is_legacy_export("FooModel"));
        assert!(registry.is_legacy_export("FooCodeGen"));
        assert!(!registry.is_legacy_export("Bar"));

        assert!(!registry.is_modern_export("Foo"));
    }

    #[test]
    fn test_model_registry_register_modern() {
        let mut registry = ModelRegistry::new();
        let definition = ModelDefinition {
            name: "Bar".to_owned(),
            source: ModelSource::Shared2023,
            definition_path: "shared_2023/models/bar.ts".into(),
            exports: smallvec!["Bar".to_owned(), "BarModel".to_owned()],
        };
        registry.register(definition);

        assert_eq!(registry.legacy_model_count(), 0);
        assert_eq!(registry.modern_model_count(), 1);
        assert_eq!(registry.modern_export_count(), 2);

        assert!(registry.is_modern_export("Bar"));
        assert!(registry.is_modern_export("BarModel"));
        assert!(!registry.is_modern_export("Foo"));

        assert!(!registry.is_legacy_export("Bar"));
    }

    #[test]
    fn test_model_registry_is_known_export() {
        let mut registry = ModelRegistry::new();

        let legacy = ModelDefinition {
            name: "Foo".to_owned(),
            source: ModelSource::SharedLegacy,
            definition_path: "shared/models/foo.ts".into(),
            exports: smallvec!["Foo".to_owned()],
        };
        registry.register(legacy);

        let modern = ModelDefinition {
            name: "Bar".to_owned(),
            source: ModelSource::Shared2023,
            definition_path: "shared_2023/models/bar.ts".into(),
            exports: smallvec!["Bar".to_owned()],
        };
        registry.register(modern);

        assert!(registry.is_known_export("Foo"));
        assert!(registry.is_known_export("Bar"));
        assert!(!registry.is_known_export("Baz"));
    }

    #[test]
    fn test_model_registry_is_export_from() {
        let mut registry = ModelRegistry::new();

        let legacy = ModelDefinition {
            name: "Foo".to_owned(),
            source: ModelSource::SharedLegacy,
            definition_path: "shared/models/foo.ts".into(),
            exports: smallvec!["Foo".to_owned()],
        };
        registry.register(legacy);

        assert!(registry.is_export_from("Foo", ModelSource::SharedLegacy));
        assert!(!registry.is_export_from("Foo", ModelSource::Shared2023));
    }

    #[test]
    fn test_model_registry_get_model() {
        let mut registry = ModelRegistry::new();

        let definition = ModelDefinition {
            name: "Foo".to_owned(),
            source: ModelSource::SharedLegacy,
            definition_path: "shared/models/foo.ts".into(),
            exports: smallvec!["Foo".to_owned()],
        };
        registry.register(definition);

        let found = registry.get_legacy_model("Foo");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Foo");

        assert!(registry.get_legacy_model("Bar").is_none());
        assert!(registry.get_modern_model("Foo").is_none());
    }

    #[test]
    fn test_model_registry_iterators() {
        let mut registry = ModelRegistry::new();

        let legacy = ModelDefinition {
            name: "Foo".to_owned(),
            source: ModelSource::SharedLegacy,
            definition_path: "shared/models/foo.ts".into(),
            exports: smallvec!["Foo".to_owned()],
        };
        registry.register(legacy);

        let modern = ModelDefinition {
            name: "Bar".to_owned(),
            source: ModelSource::Shared2023,
            definition_path: "shared_2023/models/bar.ts".into(),
            exports: smallvec!["Bar".to_owned()],
        };
        registry.register(modern);

        let legacy_names: Vec<_> = registry.iter_legacy_models().map(|d| &d.name).collect();
        assert_eq!(legacy_names, vec!["Foo"]);

        let modern_names: Vec<_> = registry.iter_modern_models().map(|d| &d.name).collect();
        assert_eq!(modern_names, vec!["Bar"]);

        let all_names: Vec<_> = registry.iter_all_models().map(|d| &d.name).collect();
        assert_eq!(all_names.len(), 2);
    }

    #[test]
    fn test_model_registry_clear() {
        let mut registry = ModelRegistry::new();

        let definition = ModelDefinition {
            name: "Foo".to_owned(),
            source: ModelSource::SharedLegacy,
            definition_path: "shared/models/foo.ts".into(),
            exports: smallvec!["Foo".to_owned()],
        };
        registry.register(definition);

        assert!(!registry.is_empty());

        registry.clear();

        assert!(registry.is_empty());
        assert_eq!(registry.total_model_count(), 0);
        assert!(!registry.is_legacy_export("Foo"));
    }
}
