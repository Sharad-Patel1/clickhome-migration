//! File information types for tracking scanned TypeScript files.
//!
//! This module provides types for representing files that have been scanned
//! for model imports, including their analysis results and migration status.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::import::ImportInfo;
use super::model::ModelReference;
use super::status::MigrationStatus;

/// An opaque identifier for a scanned file.
///
/// Uses a newtype pattern for type safety - prevents accidentally using
/// a raw integer where a file ID is expected. The inner value is typically
/// a hash of the file path for fast equality comparisons.
///
/// # Examples
///
/// ```
/// use ch_core::FileId;
///
/// let id1 = FileId(12345);
/// let id2 = FileId(12345);
/// let id3 = FileId(67890);
///
/// assert_eq!(id1, id2);
/// assert_ne!(id1, id3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u64);

impl FileId {
    /// Creates a new file ID from a u64 value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::FileId;
    ///
    /// let id = FileId::new(42);
    /// assert_eq!(id.0, 42);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the inner u64 value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::FileId;
    ///
    /// let id = FileId::new(42);
    /// assert_eq!(id.as_u64(), 42);
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl From<u64> for FileId {
    #[inline]
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<FileId> for u64 {
    #[inline]
    fn from(id: FileId) -> Self {
        id.0
    }
}

/// Information about a scanned TypeScript file.
///
/// Contains the analysis results from parsing a file, including all detected
/// imports and model references, along with metadata for change detection.
///
/// # Memory Efficiency
///
/// Uses [`SmallVec`] for imports and model references to avoid heap allocation
/// in the common case where files have fewer than 8 imports and 4 model references.
///
/// # Examples
///
/// ```
/// use ch_core::{FileInfo, FileId, MigrationStatus};
/// use camino::Utf8PathBuf;
/// use smallvec::smallvec;
///
/// let file = FileInfo {
///     id: FileId::new(1),
///     path: Utf8PathBuf::from("src/components/foo.component.ts"),
///     content_hash: 0xDEADBEEF,
///     imports: smallvec![],
///     model_refs: smallvec![],
///     status: MigrationStatus::NoModels,
///     last_scanned: 1704067200,
/// };
///
/// assert!(!file.status.needs_migration());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInfo {
    /// Unique identifier for this file.
    pub id: FileId,

    /// The file path relative to the scan root.
    pub path: Utf8PathBuf,

    /// Hash of the file contents for change detection.
    ///
    /// When a file is re-scanned, this hash is compared to determine
    /// if the content has changed and needs re-analysis.
    pub content_hash: u64,

    /// All import statements detected in the file.
    ///
    /// Uses `SmallVec<[ImportInfo; 8]>` to avoid heap allocation for
    /// files with 8 or fewer imports (the common case).
    pub imports: SmallVec<[ImportInfo; 8]>,

    /// All model references detected in the file.
    ///
    /// Uses `SmallVec<[ModelReference; 4]>` to avoid heap allocation for
    /// files with 4 or fewer model references (the common case).
    pub model_refs: SmallVec<[ModelReference; 4]>,

    /// The migration status of this file.
    pub status: MigrationStatus,

    /// Unix timestamp of when this file was last scanned.
    pub last_scanned: u64,
}

impl FileInfo {
    /// Creates a new `FileInfo` with the given path and ID.
    ///
    /// All other fields are initialized to default values (empty imports,
    /// no model refs, `NoModels` status).
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the file
    /// * `path` - The file path
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{FileInfo, FileId, MigrationStatus};
    /// use camino::Utf8PathBuf;
    ///
    /// let file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
    /// assert_eq!(file.status, MigrationStatus::NoModels);
    /// assert!(file.imports.is_empty());
    /// ```
    #[must_use]
    pub fn new(id: FileId, path: Utf8PathBuf) -> Self {
        Self {
            id,
            path,
            content_hash: 0,
            imports: SmallVec::new(),
            model_refs: SmallVec::new(),
            status: MigrationStatus::NoModels,
            last_scanned: 0,
        }
    }

    /// Returns the number of imports in this file.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{FileInfo, FileId};
    /// use camino::Utf8PathBuf;
    ///
    /// let file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
    /// assert_eq!(file.import_count(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn import_count(&self) -> usize {
        self.imports.len()
    }

    /// Returns the number of model references in this file.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{FileInfo, FileId};
    /// use camino::Utf8PathBuf;
    ///
    /// let file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
    /// assert_eq!(file.model_ref_count(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn model_ref_count(&self) -> usize {
        self.model_refs.len()
    }

    /// Returns `true` if this file needs migration work.
    ///
    /// Convenience method that delegates to [`MigrationStatus::needs_migration`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{FileInfo, FileId, MigrationStatus};
    /// use camino::Utf8PathBuf;
    ///
    /// let mut file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
    /// assert!(!file.needs_migration());
    ///
    /// file.status = MigrationStatus::Legacy;
    /// assert!(file.needs_migration());
    /// ```
    #[inline]
    #[must_use]
    pub const fn needs_migration(&self) -> bool {
        self.status.needs_migration()
    }

    /// Returns an iterator over legacy imports in this file.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::{FileInfo, FileId, ImportInfo, ImportKind, SourceLocation, ModelSource};
    /// use camino::Utf8PathBuf;
    /// use smallvec::smallvec;
    ///
    /// let mut file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
    /// file.imports = smallvec![
    ///     ImportInfo::new(
    ///         "../shared/models/foo",
    ///         ImportKind::Named,
    ///         smallvec!["Foo".to_owned()],
    ///         Some(ModelSource::SharedLegacy),
    ///         SourceLocation::default(),
    ///     ),
    /// ];
    ///
    /// assert_eq!(file.legacy_imports().count(), 1);
    /// ```
    #[inline]
    pub fn legacy_imports(&self) -> impl Iterator<Item = &ImportInfo> {
        self.imports.iter().filter(|i| i.is_legacy_import())
    }

    /// Returns an iterator over migrated imports in this file.
    #[inline]
    pub fn migrated_imports(&self) -> impl Iterator<Item = &ImportInfo> {
        self.imports
            .iter()
            .filter(|i| i.source.is_some_and(|s| !s.is_legacy()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ImportKind, ModelSource, SourceLocation};
    use smallvec::smallvec;

    #[test]
    fn test_file_id_new() {
        let id = FileId::new(42);
        assert_eq!(id.0, 42);
        assert_eq!(id.as_u64(), 42);
    }

    #[test]
    fn test_file_id_from_u64() {
        let id: FileId = 42u64.into();
        assert_eq!(id.0, 42);
    }

    #[test]
    fn test_file_id_into_u64() {
        let id = FileId::new(42);
        let value: u64 = id.into();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_file_info_new() {
        let file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
        assert_eq!(file.id, FileId::new(1));
        assert_eq!(file.path.as_str(), "src/foo.ts");
        assert_eq!(file.content_hash, 0);
        assert!(file.imports.is_empty());
        assert!(file.model_refs.is_empty());
        assert_eq!(file.status, MigrationStatus::NoModels);
        assert_eq!(file.last_scanned, 0);
    }

    #[test]
    fn test_file_info_import_count() {
        let mut file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
        assert_eq!(file.import_count(), 0);

        file.imports = smallvec![
            ImportInfo::new(
                "../shared/models/foo",
                ImportKind::Named,
                smallvec!["Foo".to_owned()],
                Some(ModelSource::SharedLegacy),
                SourceLocation::default(),
            ),
            ImportInfo::new(
                "@angular/core",
                ImportKind::Named,
                smallvec!["Component".to_owned()],
                None,
                SourceLocation::default(),
            ),
        ];
        assert_eq!(file.import_count(), 2);
    }

    #[test]
    fn test_file_info_needs_migration() {
        let mut file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
        assert!(!file.needs_migration());

        file.status = MigrationStatus::Legacy;
        assert!(file.needs_migration());

        file.status = MigrationStatus::Partial;
        assert!(file.needs_migration());

        file.status = MigrationStatus::Migrated;
        assert!(!file.needs_migration());
    }

    #[test]
    fn test_file_info_legacy_imports() {
        let mut file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
        file.imports = smallvec![
            ImportInfo::new(
                "../shared/models/foo",
                ImportKind::Named,
                smallvec!["Foo".to_owned()],
                Some(ModelSource::SharedLegacy),
                SourceLocation::default(),
            ),
            ImportInfo::new(
                "../shared_2023/models/bar",
                ImportKind::Named,
                smallvec!["Bar".to_owned()],
                Some(ModelSource::Shared2023),
                SourceLocation::default(),
            ),
            ImportInfo::new(
                "@angular/core",
                ImportKind::Named,
                smallvec!["Component".to_owned()],
                None,
                SourceLocation::default(),
            ),
        ];

        let legacy: Vec<_> = file.legacy_imports().collect();
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].path, "../shared/models/foo");

        let migrated: Vec<_> = file.migrated_imports().collect();
        assert_eq!(migrated.len(), 1);
        assert_eq!(migrated[0].path, "../shared_2023/models/bar");
    }

    #[test]
    fn test_file_info_serialization() {
        let file = FileInfo {
            id: FileId::new(42),
            path: Utf8PathBuf::from("src/components/foo.component.ts"),
            content_hash: 0xDEAD_BEEF,
            imports: smallvec![],
            model_refs: smallvec![],
            status: MigrationStatus::NoModels,
            last_scanned: 1_704_067_200,
        };

        let json = serde_json::to_string(&file).unwrap();
        let parsed: FileInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(file, parsed);
    }
}
