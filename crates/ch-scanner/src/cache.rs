//! Concurrent cache for scan results.
//!
//! This module provides [`ScanCache`], a thread-safe cache backed by
//! an `FxHashMap` guarded by a `RwLock` for storing file analysis results.
//!
//! # Concurrency Pattern
//!
//! This cache:
//!
//! - **Clones data** on `get()` operations
//! - **Uses read locks** for lookups and queries
//! - **Uses write locks** for mutations and clears
//!
//! # Examples
//!
//! ```
//! use ch_scanner::ScanCache;
//! use ch_core::{FileInfo, FileId};
//! use camino::Utf8PathBuf;
//!
//! let cache = ScanCache::new();
//!
//! // Insert a file
//! let file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
//! cache.insert(file);
//!
//! // Retrieve a clone
//! if let Some(file) = cache.get(&Utf8PathBuf::from("src/foo.ts")) {
//!     println!("Found: {}", file.path);
//! }
//! ```

use camino::{Utf8Path, Utf8PathBuf};
use ch_core::{fx_hash_map_with_capacity, FileInfo, FxHashMap, MigrationStatus};
use parking_lot::RwLock;

/// A thread-safe cache for storing [`FileInfo`] results.
///
/// Uses an `FxHashMap` guarded by a `RwLock` for concurrent access.
/// All public methods clone data to avoid exposing internal references.
///
/// # Design
///
/// The cache is keyed by file path ([`Utf8PathBuf`]) for O(1) lookups.
/// Values are [`FileInfo`] structs containing analysis results.
///
/// # Thread Safety
///
/// `ScanCache` is both `Send` and `Sync`. Multiple threads can
/// read and write concurrently without external synchronization.
///
/// # Examples
///
/// ```
/// use ch_scanner::ScanCache;
/// use ch_core::{FileInfo, FileId, MigrationStatus};
/// use camino::Utf8PathBuf;
///
/// let cache = ScanCache::new();
///
/// // Insert from multiple threads safely
/// let mut file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
/// file.status = MigrationStatus::Legacy;
/// cache.insert(file);
///
/// // Query by status
/// let legacy = cache.files_with_status(MigrationStatus::Legacy);
/// assert_eq!(legacy.len(), 1);
/// ```
#[derive(Debug, Default)]
pub struct ScanCache {
    /// The underlying concurrent map.
    files: RwLock<FxHashMap<Utf8PathBuf, FileInfo>>,
}

impl ScanCache {
    /// Creates a new empty cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    ///
    /// let cache = ScanCache::new();
    /// assert!(cache.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new cache with the specified capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - The initial capacity hint
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    ///
    /// let cache = ScanCache::with_capacity(1000);
    /// ```
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            files: RwLock::new(fx_hash_map_with_capacity(capacity)),
        }
    }

    /// Inserts a file into the cache.
    ///
    /// If a file with the same path already exists, it is replaced.
    ///
    /// # Arguments
    ///
    /// * `file` - The file info to insert
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    /// use ch_core::{FileInfo, FileId};
    /// use camino::Utf8PathBuf;
    ///
    /// let cache = ScanCache::new();
    /// let file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
    /// cache.insert(file);
    /// assert_eq!(cache.len(), 1);
    /// ```
    pub fn insert(&self, file: FileInfo) {
        self.files.write().insert(file.path.clone(), file);
    }

    /// Returns a clone of the file info for the given path, if present.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to look up
    ///
    /// # Returns
    ///
    /// A clone of the [`FileInfo`] if found, or `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    /// use ch_core::{FileInfo, FileId};
    /// use camino::Utf8PathBuf;
    ///
    /// let cache = ScanCache::new();
    /// let path = Utf8PathBuf::from("src/foo.ts");
    /// cache.insert(FileInfo::new(FileId::new(1), path.clone()));
    ///
    /// let file = cache.get(&path);
    /// assert!(file.is_some());
    /// ```
    #[must_use]
    pub fn get(&self, path: &Utf8PathBuf) -> Option<FileInfo> {
        self.files.read().get(path).cloned()
    }

    /// Returns a clone of the file info for the given path reference, if present.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to look up (as `&Utf8Path`)
    ///
    /// # Returns
    ///
    /// A clone of the [`FileInfo`] if found, or `None`.
    #[must_use]
    pub fn get_by_path(&self, path: &Utf8Path) -> Option<FileInfo> {
        self.files.read().get(path).cloned()
    }

    /// Checks if a file is in the cache.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to check
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    /// use ch_core::{FileInfo, FileId};
    /// use camino::Utf8PathBuf;
    ///
    /// let cache = ScanCache::new();
    /// let path = Utf8PathBuf::from("src/foo.ts");
    /// cache.insert(FileInfo::new(FileId::new(1), path.clone()));
    ///
    /// assert!(cache.contains(&path));
    /// ```
    #[must_use]
    pub fn contains(&self, path: &Utf8PathBuf) -> bool {
        self.files.read().contains_key(path)
    }

    /// Removes a file from the cache.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to remove
    ///
    /// # Returns
    ///
    /// The removed [`FileInfo`] if found, or `None`.
    pub fn remove(&self, path: &Utf8PathBuf) -> Option<FileInfo> {
        self.files.write().remove(path)
    }

    /// Returns the number of files in the cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    ///
    /// let cache = ScanCache::new();
    /// assert_eq!(cache.len(), 0);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.read().len()
    }

    /// Returns `true` if the cache is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    ///
    /// let cache = ScanCache::new();
    /// assert!(cache.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.read().is_empty()
    }

    /// Clears all files from the cache.
    pub fn clear(&self) {
        self.files.write().clear();
    }

    /// Checks if a file needs to be updated based on content hash.
    ///
    /// Returns `true` if:
    /// - The file is not in the cache, or
    /// - The cached file's content hash differs from the provided hash
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to check
    /// * `content_hash` - The new content hash to compare
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    /// use ch_core::{FileInfo, FileId};
    /// use camino::Utf8PathBuf;
    ///
    /// let cache = ScanCache::new();
    /// let path = Utf8PathBuf::from("src/foo.ts");
    ///
    /// // File not in cache -> needs update
    /// assert!(cache.needs_update(&path, 12345));
    ///
    /// // Insert with hash
    /// let mut file = FileInfo::new(FileId::new(1), path.clone());
    /// file.content_hash = 12345;
    /// cache.insert(file);
    ///
    /// // Same hash -> no update needed
    /// assert!(!cache.needs_update(&path, 12345));
    ///
    /// // Different hash -> needs update
    /// assert!(cache.needs_update(&path, 99999));
    /// ```
    #[must_use]
    pub fn needs_update(&self, path: &Utf8PathBuf, content_hash: u64) -> bool {
        self.files
            .read()
            .get(path)
            .is_none_or(|file| file.content_hash != content_hash)
    }

    /// Returns all files with the specified migration status.
    ///
    /// # Arguments
    ///
    /// * `status` - The status to filter by
    ///
    /// # Returns
    ///
    /// A vector of cloned [`FileInfo`] matching the status.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    /// use ch_core::{FileInfo, FileId, MigrationStatus};
    /// use camino::Utf8PathBuf;
    ///
    /// let cache = ScanCache::new();
    ///
    /// let mut file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
    /// file.status = MigrationStatus::Legacy;
    /// cache.insert(file);
    ///
    /// let legacy = cache.files_with_status(MigrationStatus::Legacy);
    /// assert_eq!(legacy.len(), 1);
    /// ```
    #[must_use]
    pub fn files_with_status(&self, status: MigrationStatus) -> Vec<FileInfo> {
        self.files
            .read()
            .values()
            .filter(|file| file.status == status)
            .cloned()
            .collect()
    }

    /// Returns all files that need migration.
    ///
    /// This includes files with [`MigrationStatus::Legacy`] or
    /// [`MigrationStatus::Partial`] status.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanCache;
    /// use ch_core::{FileInfo, FileId, MigrationStatus};
    /// use camino::Utf8PathBuf;
    ///
    /// let cache = ScanCache::new();
    ///
    /// let mut file = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/foo.ts"));
    /// file.status = MigrationStatus::Legacy;
    /// cache.insert(file);
    ///
    /// let needs_migration = cache.files_needing_migration();
    /// assert_eq!(needs_migration.len(), 1);
    /// ```
    #[must_use]
    pub fn files_needing_migration(&self) -> Vec<FileInfo> {
        self.files
            .read()
            .values()
            .filter(|file| file.status.needs_migration())
            .cloned()
            .collect()
    }

    /// Returns all files in the cache as a vector.
    ///
    /// # Returns
    ///
    /// A vector of cloned [`FileInfo`] for all cached files.
    #[must_use]
    pub fn all_files(&self) -> Vec<FileInfo> {
        self.files.read().values().cloned().collect()
    }

    /// Returns all file paths in the cache.
    ///
    /// # Returns
    ///
    /// A vector of cloned paths for all cached files.
    #[must_use]
    pub fn all_paths(&self) -> Vec<Utf8PathBuf> {
        self.files.read().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_core::FileId;

    fn make_file(id: u64, path: &str, status: MigrationStatus) -> FileInfo {
        let mut file = FileInfo::new(FileId::new(id), Utf8PathBuf::from(path));
        file.status = status;
        file
    }

    #[test]
    fn test_cache_new() {
        let cache = ScanCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_with_capacity() {
        let cache = ScanCache::with_capacity(100);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = ScanCache::new();
        let path = Utf8PathBuf::from("src/foo.ts");
        let file = FileInfo::new(FileId::new(1), path.clone());

        cache.insert(file);

        assert!(cache.contains(&path));
        let retrieved = cache.get(&path);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().map(|f| &f.path), Some(&path));
    }

    #[test]
    fn test_cache_remove() {
        let cache = ScanCache::new();
        let path = Utf8PathBuf::from("src/foo.ts");
        cache.insert(FileInfo::new(FileId::new(1), path.clone()));

        assert!(cache.contains(&path));
        let removed = cache.remove(&path);
        assert!(removed.is_some());
        assert!(!cache.contains(&path));
    }

    #[test]
    fn test_cache_clear() {
        let cache = ScanCache::new();
        cache.insert(make_file(1, "a.ts", MigrationStatus::Legacy));
        cache.insert(make_file(2, "b.ts", MigrationStatus::Migrated));

        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_needs_update() {
        let cache = ScanCache::new();
        let path = Utf8PathBuf::from("src/foo.ts");

        // Not in cache -> needs update
        assert!(cache.needs_update(&path, 12345));

        // Insert with hash
        let mut file = FileInfo::new(FileId::new(1), path.clone());
        file.content_hash = 12345;
        cache.insert(file);

        // Same hash -> no update
        assert!(!cache.needs_update(&path, 12345));

        // Different hash -> needs update
        assert!(cache.needs_update(&path, 99999));
    }

    #[test]
    fn test_cache_files_with_status() {
        let cache = ScanCache::new();
        cache.insert(make_file(1, "a.ts", MigrationStatus::Legacy));
        cache.insert(make_file(2, "b.ts", MigrationStatus::Legacy));
        cache.insert(make_file(3, "c.ts", MigrationStatus::Migrated));
        cache.insert(make_file(4, "d.ts", MigrationStatus::NoModels));

        let legacy = cache.files_with_status(MigrationStatus::Legacy);
        assert_eq!(legacy.len(), 2);

        let migrated = cache.files_with_status(MigrationStatus::Migrated);
        assert_eq!(migrated.len(), 1);

        let no_models = cache.files_with_status(MigrationStatus::NoModels);
        assert_eq!(no_models.len(), 1);
    }

    #[test]
    fn test_cache_files_needing_migration() {
        let cache = ScanCache::new();
        cache.insert(make_file(1, "a.ts", MigrationStatus::Legacy));
        cache.insert(make_file(2, "b.ts", MigrationStatus::Partial));
        cache.insert(make_file(3, "c.ts", MigrationStatus::Migrated));
        cache.insert(make_file(4, "d.ts", MigrationStatus::NoModels));

        let needs_migration = cache.files_needing_migration();
        assert_eq!(needs_migration.len(), 2);
    }

    #[test]
    fn test_cache_all_files() {
        let cache = ScanCache::new();
        cache.insert(make_file(1, "a.ts", MigrationStatus::Legacy));
        cache.insert(make_file(2, "b.ts", MigrationStatus::Migrated));

        let all = cache.all_files();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_cache_all_paths() {
        let cache = ScanCache::new();
        cache.insert(make_file(1, "a.ts", MigrationStatus::Legacy));
        cache.insert(make_file(2, "b.ts", MigrationStatus::Migrated));

        let paths = cache.all_paths();
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn test_cache_replace() {
        let cache = ScanCache::new();
        let path = Utf8PathBuf::from("src/foo.ts");

        let mut file1 = FileInfo::new(FileId::new(1), path.clone());
        file1.status = MigrationStatus::Legacy;
        cache.insert(file1);

        let mut file2 = FileInfo::new(FileId::new(1), path.clone());
        file2.status = MigrationStatus::Migrated;
        cache.insert(file2);

        let retrieved = cache.get(&path);
        assert_eq!(retrieved.map(|f| f.status), Some(MigrationStatus::Migrated));
    }
}
