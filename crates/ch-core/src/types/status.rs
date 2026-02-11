//! Migration status types.
//!
//! This module provides the [`MigrationStatus`] enum for tracking the
//! migration state of files in the codebase.

use serde::{Deserialize, Serialize};

/// The migration status of a file.
///
/// Represents whether a file has been migrated from the legacy `shared/`
/// directory to the new `shared_2023/` directory.
///
/// # Examples
///
/// ```
/// use ch_core::MigrationStatus;
///
/// let status = MigrationStatus::Legacy;
/// assert!(status.needs_migration());
///
/// let status = MigrationStatus::Migrated;
/// assert!(!status.needs_migration());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MigrationStatus {
    /// File uses only `shared/` imports and needs to be migrated.
    #[default]
    Legacy,

    /// File uses only `shared_2023/` imports and is fully migrated.
    Migrated,

    /// File uses both `shared/` and `shared_2023/` imports.
    ///
    /// This represents a partial migration state where some imports
    /// have been updated but others remain.
    Partial,

    /// File has no model imports from either shared directory.
    ///
    /// This could be a utility file, a component without model dependencies,
    /// or a file that uses models from other sources.
    NoModels,
}

impl MigrationStatus {
    /// Returns `true` if this file needs migration work.
    ///
    /// Both [`Legacy`](Self::Legacy) and [`Partial`](Self::Partial) statuses
    /// indicate that migration work is needed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::MigrationStatus;
    ///
    /// assert!(MigrationStatus::Legacy.needs_migration());
    /// assert!(MigrationStatus::Partial.needs_migration());
    /// assert!(!MigrationStatus::Migrated.needs_migration());
    /// assert!(!MigrationStatus::NoModels.needs_migration());
    /// ```
    #[inline]
    #[must_use]
    pub const fn needs_migration(self) -> bool {
        matches!(self, Self::Legacy | Self::Partial)
    }

    /// Returns `true` if this file is fully migrated.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::MigrationStatus;
    ///
    /// assert!(MigrationStatus::Migrated.is_migrated());
    /// assert!(!MigrationStatus::Legacy.is_migrated());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_migrated(self) -> bool {
        matches!(self, Self::Migrated)
    }

    /// Returns `true` if this file has any model imports.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::MigrationStatus;
    ///
    /// assert!(MigrationStatus::Legacy.has_models());
    /// assert!(MigrationStatus::Migrated.has_models());
    /// assert!(!MigrationStatus::NoModels.has_models());
    /// ```
    #[inline]
    #[must_use]
    pub const fn has_models(self) -> bool {
        !matches!(self, Self::NoModels)
    }

    /// Returns a human-readable label for this status.
    ///
    /// Useful for display in the TUI.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_core::MigrationStatus;
    ///
    /// assert_eq!(MigrationStatus::Legacy.label(), "Legacy");
    /// assert_eq!(MigrationStatus::Migrated.label(), "Migrated");
    /// ```
    #[inline]
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Legacy => "Legacy",
            Self::Migrated => "Migrated",
            Self::Partial => "Partial",
            Self::NoModels => "No Models",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_status_needs_migration() {
        assert!(MigrationStatus::Legacy.needs_migration());
        assert!(MigrationStatus::Partial.needs_migration());
        assert!(!MigrationStatus::Migrated.needs_migration());
        assert!(!MigrationStatus::NoModels.needs_migration());
    }

    #[test]
    fn test_migration_status_is_migrated() {
        assert!(!MigrationStatus::Legacy.is_migrated());
        assert!(!MigrationStatus::Partial.is_migrated());
        assert!(MigrationStatus::Migrated.is_migrated());
        assert!(!MigrationStatus::NoModels.is_migrated());
    }

    #[test]
    fn test_migration_status_has_models() {
        assert!(MigrationStatus::Legacy.has_models());
        assert!(MigrationStatus::Partial.has_models());
        assert!(MigrationStatus::Migrated.has_models());
        assert!(!MigrationStatus::NoModels.has_models());
    }

    #[test]
    fn test_migration_status_labels() {
        assert_eq!(MigrationStatus::Legacy.label(), "Legacy");
        assert_eq!(MigrationStatus::Migrated.label(), "Migrated");
        assert_eq!(MigrationStatus::Partial.label(), "Partial");
        assert_eq!(MigrationStatus::NoModels.label(), "No Models");
    }

    #[test]
    fn test_migration_status_default() {
        assert_eq!(MigrationStatus::default(), MigrationStatus::Legacy);
    }

    #[test]
    fn test_migration_status_serialization() {
        assert_eq!(
            serde_json::to_string(&MigrationStatus::Legacy).unwrap(),
            r#""legacy""#
        );
        assert_eq!(
            serde_json::to_string(&MigrationStatus::Migrated).unwrap(),
            r#""migrated""#
        );
        assert_eq!(
            serde_json::to_string(&MigrationStatus::Partial).unwrap(),
            r#""partial""#
        );
        assert_eq!(
            serde_json::to_string(&MigrationStatus::NoModels).unwrap(),
            r#""no_models""#
        );
    }

    #[test]
    fn test_migration_status_deserialization() {
        let status: MigrationStatus = serde_json::from_str(r#""legacy""#).unwrap();
        assert_eq!(status, MigrationStatus::Legacy);

        let status: MigrationStatus = serde_json::from_str(r#""migrated""#).unwrap();
        assert_eq!(status, MigrationStatus::Migrated);
    }
}
