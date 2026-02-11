//! Import information types for tracking TypeScript imports.
//!
//! This module provides types for representing import statements detected
//! in TypeScript files during scanning.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::location::SourceLocation;
use super::model::ModelSource;

/// The kind of import statement.
///
/// TypeScript supports various import syntaxes, each represented by a variant
/// of this enum.
///
/// # Examples
///
/// ```
/// use ch_core::ImportKind;
///
/// let kind = ImportKind::Named;
/// assert!(!kind.is_dynamic());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ImportKind {
    /// Named imports: `import { Foo, Bar } from '...'`
    Named,

    /// Default import: `import Foo from '...'`
    Default,

    /// Namespace import: `import * as Foo from '...'`
    Namespace,

    /// Side-effect import: `import '...'`
    SideEffect,

    /// Type-only import: `import type { Foo } from '...'`
    TypeOnly,

    /// Dynamic import: `await import('...')`
    Dynamic,
}

impl ImportKind {
    /// Returns `true` if this is a dynamic import.
    ///
    /// Dynamic imports are handled differently from static imports
    /// as they occur at runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::ImportKind;
    ///
    /// assert!(ImportKind::Dynamic.is_dynamic());
    /// assert!(!ImportKind::Named.is_dynamic());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_dynamic(self) -> bool {
        matches!(self, Self::Dynamic)
    }

    /// Returns `true` if this import brings names into scope.
    ///
    /// Side-effect imports don't import any names.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::ImportKind;
    ///
    /// assert!(ImportKind::Named.has_bindings());
    /// assert!(!ImportKind::SideEffect.has_bindings());
    /// ```
    #[inline]
    #[must_use]
    pub const fn has_bindings(self) -> bool {
        !matches!(self, Self::SideEffect)
    }

    /// Returns `true` if this is a type-only import.
    ///
    /// Type-only imports are erased at runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::ImportKind;
    ///
    /// assert!(ImportKind::TypeOnly.is_type_only());
    /// assert!(!ImportKind::Named.is_type_only());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_type_only(self) -> bool {
        matches!(self, Self::TypeOnly)
    }
}

/// Information about an import statement in a TypeScript file.
///
/// Captures all relevant details about an import, including the module path,
/// imported names, and source location.
///
/// # Examples
///
/// ```
/// use ch_core::{ImportInfo, ImportKind, SourceLocation, ModelSource};
/// use smallvec::smallvec;
///
/// let import = ImportInfo {
///     path: "../shared/models/active-contract".to_owned(),
///     kind: ImportKind::Named,
///     names: smallvec!["ActiveContract".to_owned(), "ActiveContractForm".to_owned()],
///     source: Some(ModelSource::SharedLegacy),
///     location: SourceLocation::new(5, 0, 120),
/// };
///
/// assert_eq!(import.names.len(), 2);
/// assert!(import.source.is_some());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportInfo {
    /// The module path from the import statement.
    ///
    /// This is the raw path as it appears in the source code,
    /// e.g., `"../shared/models/active-contract"`.
    pub path: String,

    /// The kind of import statement.
    pub kind: ImportKind,

    /// The names imported from the module.
    ///
    /// For named imports, this contains the individual imported names.
    /// For default/namespace imports, this contains the local binding name.
    /// For side-effect imports, this is empty.
    ///
    /// Uses `SmallVec` for stack allocation when there are 4 or fewer names,
    /// which covers the majority of import statements.
    pub names: SmallVec<[String; 4]>,

    /// The detected model source, if this import is from a shared directory.
    ///
    /// `None` if the import is not from `shared/` or `shared_2023/`.
    pub source: Option<ModelSource>,

    /// The location of the import statement in the source file.
    pub location: SourceLocation,
}

impl ImportInfo {
    /// Creates a new import info.
    ///
    /// # Arguments
    ///
    /// * `path` - The module path from the import statement
    /// * `kind` - The kind of import
    /// * `names` - The imported names
    /// * `source` - The model source (if applicable)
    /// * `location` - The source location
    #[must_use]
    pub fn new(
        path: impl Into<String>,
        kind: ImportKind,
        names: SmallVec<[String; 4]>,
        source: Option<ModelSource>,
        location: SourceLocation,
    ) -> Self {
        Self {
            path: path.into(),
            kind,
            names,
            source,
            location,
        }
    }

    /// Returns `true` if this import is from a shared model directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{ImportInfo, ImportKind, SourceLocation, ModelSource};
    /// use smallvec::smallvec;
    ///
    /// let shared_import = ImportInfo {
    ///     path: "../shared/models/foo".to_owned(),
    ///     kind: ImportKind::Named,
    ///     names: smallvec!["Foo".to_owned()],
    ///     source: Some(ModelSource::SharedLegacy),
    ///     location: SourceLocation::default(),
    /// };
    /// assert!(shared_import.is_model_import());
    ///
    /// let other_import = ImportInfo {
    ///     path: "@angular/core".to_owned(),
    ///     kind: ImportKind::Named,
    ///     names: smallvec!["Component".to_owned()],
    ///     source: None,
    ///     location: SourceLocation::default(),
    /// };
    /// assert!(!other_import.is_model_import());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_model_import(&self) -> bool {
        self.source.is_some()
    }

    /// Returns `true` if this import is from the legacy shared directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{ImportInfo, ImportKind, SourceLocation, ModelSource};
    /// use smallvec::smallvec;
    ///
    /// let legacy_import = ImportInfo {
    ///     path: "../shared/models/foo".to_owned(),
    ///     kind: ImportKind::Named,
    ///     names: smallvec!["Foo".to_owned()],
    ///     source: Some(ModelSource::SharedLegacy),
    ///     location: SourceLocation::default(),
    /// };
    /// assert!(legacy_import.is_legacy_import());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_legacy_import(&self) -> bool {
        self.source.is_some_and(ModelSource::is_legacy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn test_import_kind_is_dynamic() {
        assert!(ImportKind::Dynamic.is_dynamic());
        assert!(!ImportKind::Named.is_dynamic());
        assert!(!ImportKind::Default.is_dynamic());
        assert!(!ImportKind::Namespace.is_dynamic());
        assert!(!ImportKind::SideEffect.is_dynamic());
        assert!(!ImportKind::TypeOnly.is_dynamic());
    }

    #[test]
    fn test_import_kind_has_bindings() {
        assert!(ImportKind::Named.has_bindings());
        assert!(ImportKind::Default.has_bindings());
        assert!(ImportKind::Namespace.has_bindings());
        assert!(!ImportKind::SideEffect.has_bindings());
        assert!(ImportKind::TypeOnly.has_bindings());
        assert!(ImportKind::Dynamic.has_bindings());
    }

    #[test]
    fn test_import_kind_is_type_only() {
        assert!(ImportKind::TypeOnly.is_type_only());
        assert!(!ImportKind::Named.is_type_only());
    }

    #[test]
    fn test_import_info_new() {
        let import = ImportInfo::new(
            "../shared/models/foo",
            ImportKind::Named,
            smallvec!["Foo".to_owned()],
            Some(ModelSource::SharedLegacy),
            SourceLocation::new(1, 0, 0),
        );
        assert_eq!(import.path, "../shared/models/foo");
        assert_eq!(import.kind, ImportKind::Named);
        assert_eq!(import.names.len(), 1);
        assert!(import.is_model_import());
    }

    #[test]
    fn test_import_info_is_model_import() {
        let model_import = ImportInfo {
            path: "../shared/models/foo".to_owned(),
            kind: ImportKind::Named,
            names: smallvec!["Foo".to_owned()],
            source: Some(ModelSource::SharedLegacy),
            location: SourceLocation::default(),
        };
        assert!(model_import.is_model_import());

        let non_model_import = ImportInfo {
            path: "@angular/core".to_owned(),
            kind: ImportKind::Named,
            names: smallvec!["Component".to_owned()],
            source: None,
            location: SourceLocation::default(),
        };
        assert!(!non_model_import.is_model_import());
    }

    #[test]
    fn test_import_info_is_legacy_import() {
        let legacy = ImportInfo {
            path: "../shared/models/foo".to_owned(),
            kind: ImportKind::Named,
            names: smallvec!["Foo".to_owned()],
            source: Some(ModelSource::SharedLegacy),
            location: SourceLocation::default(),
        };
        assert!(legacy.is_legacy_import());

        let new = ImportInfo {
            path: "../shared_2023/models/foo".to_owned(),
            kind: ImportKind::Named,
            names: smallvec!["Foo".to_owned()],
            source: Some(ModelSource::Shared2023),
            location: SourceLocation::default(),
        };
        assert!(!new.is_legacy_import());

        let none = ImportInfo {
            path: "@angular/core".to_owned(),
            kind: ImportKind::Named,
            names: smallvec!["Component".to_owned()],
            source: None,
            location: SourceLocation::default(),
        };
        assert!(!none.is_legacy_import());
    }

    #[test]
    fn test_import_info_serialization() {
        let import = ImportInfo {
            path: "../shared/models/foo".to_owned(),
            kind: ImportKind::Named,
            names: smallvec!["Foo".to_owned(), "Bar".to_owned()],
            source: Some(ModelSource::SharedLegacy),
            location: SourceLocation::new(10, 5, 245),
        };
        let json = serde_json::to_string(&import).unwrap();
        let parsed: ImportInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(import, parsed);
    }

    #[test]
    fn test_smallvec_stack_allocation() {
        // SmallVec<[String; 4]> should use stack allocation for <= 4 elements
        let names: SmallVec<[String; 4]> = smallvec![
            "A".to_owned(),
            "B".to_owned(),
            "C".to_owned(),
            "D".to_owned()
        ];
        assert_eq!(names.len(), 4);
        // Note: We can't easily test if it's on the stack, but we verify it works
    }
}
