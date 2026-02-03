//! Model-related types for tracking TypeScript model references.
//!
//! This module provides types for representing model artifacts in the `ClickHome`
//! codebase, including their source (legacy vs new) and category (interface,
//! codegen, form, etc.).

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
