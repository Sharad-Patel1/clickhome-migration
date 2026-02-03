//! Filesystem scanner for TypeScript files with parallel analysis.
//!
//! This crate is the parallel file discovery and analysis engine for the
//! ch-migration TUI. It scans TypeScript files, parses them using
//! `ch-ts-parser`, and caches results for efficient access.
//!
//! # Overview
//!
//! The main entry point is [`Scanner`], which combines:
//!
//! - [`FileWalker`]: Directory traversal respecting `.gitignore` patterns
//! - [`FileAnalyzer`]: Parallel file processing with rayon + bumpalo arenas
//! - [`ScanCache`]: Concurrent caching with `DashMap`
//! - [`ScanStats`]: Atomic statistics for progress tracking
//!
//! # Example
//!
//! ```ignore
//! use ch_scanner::{Scanner, ScanConfig};
//! use camino::Utf8Path;
//!
//! let config = ScanConfig::new(Utf8Path::new("./src"));
//! let scanner = Scanner::new(config)?;
//!
//! // Perform initial scan
//! let result = scanner.scan()?;
//! println!("Scanned {} files", result.stats.total);
//!
//! // Access cached results
//! for file in scanner.files_needing_migration() {
//!     println!("Needs migration: {}", file.path);
//! }
//!
//! // Get statistics
//! let stats = scanner.stats();
//! println!("Progress: {:.1}%", stats.progress_percent());
//! ```
//!
//! # Architecture
//!
//! ```text
//! Scanner (main entry point)
//!     │
//!     ├── FileWalker (collect paths)
//!     │       │
//!     │       └── WalkBuilder (ignore crate)
//!     │
//!     ├── FileAnalyzer (parallel parsing)
//!     │       │
//!     │       ├── Herd (per-thread arenas)
//!     │       └── ArenaParser (ch-ts-parser)
//!     │
//!     ├── ScanCache (DashMap storage)
//!     │
//!     └── ScanStats (atomic counters)
//! ```
//!
//! # Performance
//!
//! - **Memory**: O(files) for path collection, arena reset after each parse
//! - **CPU**: O(files/threads) with rayon work-stealing
//! - **Locking**: Zero contention - atomics for stats, per-thread arenas

#![deny(clippy::all)]
#![warn(missing_docs)]

mod analyzer;
mod cache;
mod error;
mod stats;
mod walker;

pub use analyzer::FileAnalyzer;
pub use cache::ScanCache;
pub use error::ScanError;
pub use stats::{ScanStats, StatsSnapshot};
pub use walker::FileWalker;

use camino::{Utf8Path, Utf8PathBuf};
use ch_core::{FileInfo, MigrationStatus};
use tracing::{debug, info, warn};

/// Configuration for the scanner.
///
/// # Examples
///
/// ```
/// use ch_scanner::ScanConfig;
/// use camino::Utf8Path;
///
/// let config = ScanConfig::new(Utf8Path::new("./src"))
///     .with_skip_dirs(&["vendor", "third_party"]);
/// ```
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// Root directory to scan.
    pub root: Utf8PathBuf,
    /// Additional directories to skip.
    pub skip_dirs: Vec<String>,
    /// Whether to follow symbolic links.
    pub follow_links: bool,
}

impl ScanConfig {
    /// Creates a new scan configuration with the given root directory.
    ///
    /// # Arguments
    ///
    /// * `root` - The root directory to scan
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanConfig;
    /// use camino::Utf8Path;
    ///
    /// let config = ScanConfig::new(Utf8Path::new("./src"));
    /// ```
    #[must_use]
    pub fn new(root: &Utf8Path) -> Self {
        Self {
            root: root.to_owned(),
            skip_dirs: Vec::new(),
            follow_links: false,
        }
    }

    /// Adds directories to skip during scanning.
    ///
    /// # Arguments
    ///
    /// * `dirs` - Directory names to skip
    #[must_use]
    pub fn with_skip_dirs(mut self, dirs: &[&str]) -> Self {
        self.skip_dirs.extend(dirs.iter().map(ToString::to_string));
        self
    }

    /// Configures whether to follow symbolic links.
    ///
    /// # Arguments
    ///
    /// * `follow` - Whether to follow symbolic links
    #[must_use]
    pub const fn with_follow_links(mut self, follow: bool) -> Self {
        self.follow_links = follow;
        self
    }
}

/// Result of a scan operation.
///
/// Contains statistics and any non-fatal errors encountered.
#[derive(Debug)]
pub struct ScanResult {
    /// Statistics snapshot from the scan.
    pub stats: StatsSnapshot,
    /// Non-fatal errors encountered during scanning.
    pub errors: Vec<(Utf8PathBuf, ScanError)>,
}

/// The main scanner for TypeScript files.
///
/// Combines file walking, parallel analysis, caching, and statistics
/// into a single interface.
///
/// # Examples
///
/// ```ignore
/// use ch_scanner::{Scanner, ScanConfig};
/// use camino::Utf8Path;
///
/// let config = ScanConfig::new(Utf8Path::new("./src"));
/// let scanner = Scanner::new(config)?;
///
/// // Initial scan
/// let result = scanner.scan()?;
///
/// // Access files by status
/// for file in scanner.files_with_status(MigrationStatus::Legacy) {
///     println!("{}", file.path);
/// }
/// ```
#[derive(Debug)]
pub struct Scanner {
    /// Scanner configuration.
    config: ScanConfig,
    /// File analysis results cache.
    cache: ScanCache,
    /// Statistics counters.
    stats: ScanStats,
}

impl Scanner {
    /// Creates a new scanner with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - The scanner configuration
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::Config`] if the configuration is invalid
    /// (e.g., root directory doesn't exist).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use ch_scanner::{Scanner, ScanConfig};
    /// use camino::Utf8Path;
    ///
    /// let config = ScanConfig::new(Utf8Path::new("./src"));
    /// let scanner = Scanner::new(config)?;
    /// ```
    pub fn new(config: ScanConfig) -> Result<Self, ScanError> {
        // Validate configuration
        if !config.root.exists() {
            return Err(ScanError::config(format!(
                "root path does not exist: {}",
                config.root
            )));
        }

        if !config.root.is_dir() {
            return Err(ScanError::config(format!(
                "root path is not a directory: {}",
                config.root
            )));
        }

        info!(root = %config.root, "Creating scanner");

        Ok(Self {
            config,
            cache: ScanCache::new(),
            stats: ScanStats::new(),
        })
    }

    /// Performs a full scan of the configured directory.
    ///
    /// This method:
    /// 1. Walks the directory tree to collect TypeScript file paths
    /// 2. Analyzes files in parallel using rayon
    /// 3. Updates the cache with results
    /// 4. Updates statistics counters
    ///
    /// # Returns
    ///
    /// A [`ScanResult`] containing statistics and any non-fatal errors.
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::Walk`] if directory traversal fails.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let result = scanner.scan()?;
    /// println!("Scanned {} files", result.stats.total);
    /// ```
    pub fn scan(&self) -> Result<ScanResult, ScanError> {
        info!(root = %self.config.root, "Starting scan");

        // Reset statistics for fresh scan
        self.stats.reset();
        self.cache.clear();

        // Walk directory to collect paths
        let walker = self.build_walker()?;
        let paths = walker.collect_paths()?;

        info!(count = paths.len(), "Collected TypeScript files");

        // Analyze files in parallel
        let analyzer = FileAnalyzer::new();
        let results = analyzer.analyze_files(&paths);

        // Process results
        let mut errors = Vec::new();

        for (path, result) in results {
            self.stats.increment_total();

            match result {
                Ok(file_info) => {
                    // Update statistics based on status
                    match file_info.status {
                        MigrationStatus::Legacy => self.stats.increment_legacy(),
                        MigrationStatus::Migrated => self.stats.increment_migrated(),
                        MigrationStatus::Partial => self.stats.increment_partial(),
                        MigrationStatus::NoModels => self.stats.increment_no_models(),
                        _ => {} // Handle any future status variants
                    }

                    debug!(path = %file_info.path, status = ?file_info.status, "Analyzed file");
                    self.cache.insert(file_info);
                }
                Err(e) => {
                    self.stats.increment_errors();
                    warn!(path = %path, error = %e, "Failed to analyze file");
                    errors.push((path, e));
                }
            }
        }

        let stats = self.stats.snapshot();
        info!(
            total = stats.total,
            legacy = stats.legacy,
            migrated = stats.migrated,
            partial = stats.partial,
            errors = stats.errors,
            "Scan completed"
        );

        Ok(ScanResult { stats, errors })
    }

    /// Re-scans specific files.
    ///
    /// This is more efficient than a full scan when only a few files
    /// have changed (e.g., from file watching).
    ///
    /// # Arguments
    ///
    /// * `paths` - The file paths to re-scan
    ///
    /// # Returns
    ///
    /// A vector of `(path, Result<(), ScanError>)` indicating success/failure
    /// for each file.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let results = scanner.rescan_files(&[
    ///     Utf8PathBuf::from("src/foo.ts"),
    ///     Utf8PathBuf::from("src/bar.ts"),
    /// ]);
    /// ```
    pub fn rescan_files(&self, paths: &[Utf8PathBuf]) -> Vec<(Utf8PathBuf, Result<(), ScanError>)> {
        debug!(count = paths.len(), "Re-scanning files");

        let analyzer = FileAnalyzer::new();
        let results = analyzer.analyze_files(paths);

        results
            .into_iter()
            .map(|(path, result)| {
                let outcome = match result {
                    Ok(file_info) => {
                        // Update cache and statistics
                        // Note: We don't decrement old status since we'd need to track it
                        match file_info.status {
                            MigrationStatus::Legacy => self.stats.increment_legacy(),
                            MigrationStatus::Migrated => self.stats.increment_migrated(),
                            MigrationStatus::Partial => self.stats.increment_partial(),
                            MigrationStatus::NoModels => self.stats.increment_no_models(),
                            _ => {} // Handle any future status variants
                        }
                        self.cache.insert(file_info);
                        Ok(())
                    }
                    Err(e) => {
                        self.stats.increment_errors();
                        Err(e)
                    }
                };
                (path, outcome)
            })
            .collect()
    }

    /// Returns a snapshot of current statistics.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let stats = scanner.stats();
    /// println!("Progress: {:.1}%", stats.progress_percent());
    /// ```
    #[must_use]
    pub fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }

    /// Returns a clone of the file info for the given path, if cached.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to look up
    ///
    /// # Examples
    ///
    /// ```ignore
    /// if let Some(file) = scanner.get_file(Utf8Path::new("src/foo.ts")) {
    ///     println!("Status: {:?}", file.status);
    /// }
    /// ```
    #[must_use]
    pub fn get_file(&self, path: &Utf8Path) -> Option<FileInfo> {
        self.cache.get_by_path(path)
    }

    /// Returns all files with the specified migration status.
    ///
    /// # Arguments
    ///
    /// * `status` - The status to filter by
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let legacy = scanner.files_with_status(MigrationStatus::Legacy);
    /// println!("Found {} legacy files", legacy.len());
    /// ```
    #[must_use]
    pub fn files_with_status(&self, status: MigrationStatus) -> Vec<FileInfo> {
        self.cache.files_with_status(status)
    }

    /// Returns all files that need migration.
    ///
    /// Includes files with [`MigrationStatus::Legacy`] or
    /// [`MigrationStatus::Partial`] status.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// for file in scanner.files_needing_migration() {
    ///     println!("{}: {:?}", file.path, file.status);
    /// }
    /// ```
    #[must_use]
    pub fn files_needing_migration(&self) -> Vec<FileInfo> {
        self.cache.files_needing_migration()
    }

    /// Returns a reference to the underlying cache.
    ///
    /// This provides direct access to the cache for advanced queries.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let cache = scanner.cache();
    /// let all_files = cache.all_files();
    /// ```
    #[must_use]
    pub const fn cache(&self) -> &ScanCache {
        &self.cache
    }

    /// Returns the scanner configuration.
    #[must_use]
    pub const fn config(&self) -> &ScanConfig {
        &self.config
    }

    /// Builds a file walker with the current configuration.
    fn build_walker(&self) -> Result<FileWalker, ScanError> {
        let mut walker = FileWalker::new(&self.config.root)?;

        if !self.config.skip_dirs.is_empty() {
            let skip_dirs: Vec<&str> = self.config.skip_dirs.iter().map(String::as_str).collect();
            walker = walker.with_skip_dirs(&skip_dirs);
        }

        walker = walker.with_follow_links(self.config.follow_links);

        Ok(walker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_config_new() {
        let config = ScanConfig::new(Utf8Path::new("./src"));
        assert_eq!(config.root.as_str(), "./src");
        assert!(config.skip_dirs.is_empty());
        assert!(!config.follow_links);
    }

    #[test]
    fn test_scan_config_with_skip_dirs() {
        let config = ScanConfig::new(Utf8Path::new("./src")).with_skip_dirs(&["vendor", "lib"]);

        assert_eq!(config.skip_dirs.len(), 2);
        assert!(config.skip_dirs.contains(&"vendor".to_owned()));
        assert!(config.skip_dirs.contains(&"lib".to_owned()));
    }

    #[test]
    fn test_scan_config_with_follow_links() {
        let config = ScanConfig::new(Utf8Path::new("./src")).with_follow_links(true);
        assert!(config.follow_links);
    }

    #[test]
    fn test_scanner_invalid_root() {
        let config = ScanConfig::new(Utf8Path::new("/nonexistent/path/that/does/not/exist"));
        let result = Scanner::new(config);
        assert!(result.is_err());
    }
}
