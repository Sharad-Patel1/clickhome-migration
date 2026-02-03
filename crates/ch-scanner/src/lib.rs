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
//! - [`ScanCache`]: Concurrent caching with `FxHashMap` + `RwLock`
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
//! # Streaming API
//!
//! For large codebases, use the streaming API to receive results as they're
//! processed, enabling live UI updates:
//!
//! ```ignore
//! use ch_scanner::{Scanner, ScanConfig, ScanUpdate};
//! use tokio::sync::mpsc;
//!
//! let (tx, mut rx) = mpsc::channel(256);
//! let scanner = Scanner::new(ScanConfig::new(Utf8Path::new("./src")))?;
//!
//! // Spawn blocking scan in background
//! let scanner_clone = scanner.clone();
//! tokio::task::spawn_blocking(move || {
//!     scanner_clone.scan_streaming(tx).ok();
//! });
//!
//! // Process updates as they arrive
//! while let Some(update) = rx.recv().await {
//!     match update {
//!         ScanUpdate::PathsDiscovered(count) => println!("Found {} files", count),
//!         ScanUpdate::FileScanned(info) => println!("Scanned: {}", info.path),
//!         ScanUpdate::FileError { path, .. } => println!("Error: {}", path),
//!         ScanUpdate::Complete(result) => println!("Done: {} total", result.stats.total),
//!     }
//! }
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
//!     ├── ScanCache (FxHashMap + RwLock)
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
mod registry;
mod stats;
mod walker;

pub use analyzer::FileAnalyzer;
pub use cache::ScanCache;
pub use error::ScanError;
pub use registry::{RegistryBuildResult, RegistryBuilder};
pub use stats::{ScanStats, StatsSnapshot};
pub use walker::FileWalker;

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use ch_core::{FileInfo, MigrationStatus, ModelRegistry};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use ch_ts_parser::ModelPathMatcher;

/// Update sent during a streaming scan operation.
///
/// These updates allow the TUI to display progress in real-time as files
/// are discovered and analyzed, rather than waiting for the entire scan
/// to complete.
///
/// # Size Optimization
///
/// The `FileScanned` variant is boxed to reduce enum size, since
/// `FileInfo` is much larger than other variants. This improves
/// channel throughput during streaming scans.
#[derive(Debug)]
pub enum ScanUpdate {
    /// Total paths discovered (sent once after directory walk completes).
    ///
    /// Use this to pre-allocate storage and show "Scanning N files..."
    PathsDiscovered(usize),

    /// A single file was successfully analyzed.
    ///
    /// Sent immediately after each file is parsed, enabling live updates.
    /// Boxed to reduce enum size for efficient channel transmission.
    FileScanned(Box<FileInfo>),

    /// A single file failed to analyze.
    ///
    /// Contains the path and error for logging/display purposes.
    FileError {
        /// The path of the file that failed.
        path: Utf8PathBuf,
        /// The error that occurred.
        error: ScanError,
    },

    /// Scan completed with final statistics.
    ///
    /// Sent after all files have been processed. The result contains
    /// the final statistics snapshot and any accumulated errors.
    Complete(ScanResult),
}

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
    /// Path to the legacy shared directory (for building model registry).
    pub shared_path: Option<Utf8PathBuf>,
    /// Path to the modern `shared_2023` directory (for building model registry).
    pub shared_2023_path: Option<Utf8PathBuf>,
    /// Whether to build the model registry for import filtering.
    pub use_registry: bool,
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
            shared_path: None,
            shared_2023_path: None,
            use_registry: false,
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

    /// Configures the paths to the shared directories for building the model registry.
    ///
    /// When set, the scanner will build a model registry and use it to filter
    /// imports, ensuring only actual model exports are tracked.
    ///
    /// # Arguments
    ///
    /// * `shared` - Path to the legacy `shared/` directory
    /// * `shared_2023` - Path to the modern `shared_2023/` directory
    #[must_use]
    pub fn with_shared_paths(mut self, shared: &Utf8Path, shared_2023: &Utf8Path) -> Self {
        self.shared_path = Some(shared.to_owned());
        self.shared_2023_path = Some(shared_2023.to_owned());
        self.use_registry = true;
        self
    }

    /// Enables or disables registry-based import filtering.
    ///
    /// When enabled (and shared paths are set), imports are validated against
    /// the model registry to ensure only actual model exports are tracked.
    #[must_use]
    pub const fn with_registry(mut self, use_registry: bool) -> Self {
        self.use_registry = use_registry;
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
/// # Registry-Based Filtering
///
/// When configured with shared directory paths, the scanner builds a
/// [`ModelRegistry`] during initialization. This registry is used to
/// filter imports, ensuring only actual model exports are tracked.
///
/// # Cloning
///
/// `Scanner` is cheaply cloneable via internal `Arc` references.
/// Clones share the same cache, statistics, and registry, enabling use from
/// background tasks while the main thread accesses results.
///
/// # Examples
///
/// ```ignore
/// use ch_scanner::{Scanner, ScanConfig};
/// use camino::Utf8Path;
///
/// let config = ScanConfig::new(Utf8Path::new("./src"))
///     .with_shared_paths(
///         Utf8Path::new("./src/shared"),
///         Utf8Path::new("./src/shared_2023"),
///     );
/// let scanner = Scanner::new(config)?;
///
/// // Initial scan
/// let result = scanner.scan()?;
///
/// // Access files by status
/// for file in scanner.files_with_status(MigrationStatus::Legacy) {
///     println!("{}", file.path);
/// }
///
/// // Access the registry for model information
/// let registry = scanner.registry();
/// println!("Legacy models: {}", registry.legacy_model_count());
/// ```
#[derive(Debug, Clone)]
pub struct Scanner {
    /// Scanner configuration.
    config: ScanConfig,
    /// Model path matcher for import detection.
    model_path_matcher: ModelPathMatcher,
    /// Model registry for filtering imports (shared via Arc for cloning).
    registry: Arc<ModelRegistry>,
    /// File analysis results cache (shared via Arc for cloning).
    cache: Arc<ScanCache>,
    /// Statistics counters (shared via Arc for cloning).
    stats: Arc<ScanStats>,
}

impl Scanner {
    /// Creates a new scanner with the given configuration.
    ///
    /// If shared directory paths are configured and registry is enabled,
    /// this will build the model registry during initialization.
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
    /// Returns [`ScanError::Registry`] if registry building fails (when enabled).
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
        Self::new_with_matcher(config, ModelPathMatcher::default())
    }

    /// Creates a new scanner with a custom model path matcher.
    ///
    /// # Arguments
    ///
    /// * `config` - The scanner configuration
    /// * `matcher` - Model path matcher for import detection
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::Config`] if the configuration is invalid
    /// (e.g., root directory doesn't exist).
    ///
    /// Returns [`ScanError::Registry`] if registry building fails (when enabled).
    pub fn new_with_matcher(
        config: ScanConfig,
        matcher: ModelPathMatcher,
    ) -> Result<Self, ScanError> {
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

        // Build model registry if configured
        let registry = if config.use_registry {
            if let (Some(shared), Some(shared_2023)) =
                (&config.shared_path, &config.shared_2023_path)
            {
                info!(
                    shared = %shared,
                    shared_2023 = %shared_2023,
                    "Building model registry"
                );
                let builder = RegistryBuilder::new(shared, shared_2023);
                builder.build()?
            } else {
                warn!("Registry enabled but shared paths not configured, using empty registry");
                ModelRegistry::new()
            }
        } else {
            ModelRegistry::new()
        };

        info!(
            root = %config.root,
            use_registry = config.use_registry,
            legacy_models = registry.legacy_model_count(),
            modern_models = registry.modern_model_count(),
            "Creating scanner"
        );

        Ok(Self {
            config,
            model_path_matcher: matcher,
            registry: Arc::new(registry),
            cache: Arc::new(ScanCache::new()),
            stats: Arc::new(ScanStats::new()),
        })
    }

    /// Creates a new scanner with a pre-built registry.
    ///
    /// Use this when you want to share a registry across multiple scanners
    /// or when you've built the registry separately.
    ///
    /// # Arguments
    ///
    /// * `config` - The scanner configuration
    /// * `matcher` - Model path matcher for import detection
    /// * `registry` - Pre-built model registry
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::Config`] if the configuration is invalid.
    pub fn new_with_registry(
        config: ScanConfig,
        matcher: ModelPathMatcher,
        registry: Arc<ModelRegistry>,
    ) -> Result<Self, ScanError> {
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

        info!(
            root = %config.root,
            legacy_models = registry.legacy_model_count(),
            modern_models = registry.modern_model_count(),
            "Creating scanner with pre-built registry"
        );

        Ok(Self {
            config,
            model_path_matcher: matcher,
            registry,
            cache: Arc::new(ScanCache::new()),
            stats: Arc::new(ScanStats::new()),
        })
    }

    /// Performs a full scan of the configured directory.
    ///
    /// This method:
    /// 1. Walks the directory tree to collect TypeScript file paths
    /// 2. Analyzes files in parallel using rayon
    /// 3. Filters imports against the registry (if enabled)
    /// 4. Updates the cache with results
    /// 5. Updates statistics counters
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

        // Determine registry reference for filtering
        let registry_ref = if self.config.use_registry {
            Some(self.registry.as_ref())
        } else {
            None
        };

        // Analyze files in parallel
        let analyzer = FileAnalyzer::new();
        let results = analyzer.analyze_files(&paths, &self.model_path_matcher, registry_ref);

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

    /// Performs a streaming scan, sending results via channel.
    ///
    /// Unlike [`scan()`](Self::scan), this method streams results as they become
    /// available, enabling live UI updates during the scan. Each file result is
    /// sent immediately after analysis completes.
    ///
    /// # Arguments
    ///
    /// * `tx` - Channel sender for streaming updates (takes ownership)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful completion, or `Err` if the directory
    /// walk fails. Individual file errors are sent via the channel as
    /// [`ScanUpdate::FileError`] rather than causing the method to fail.
    ///
    /// # Channel Protocol
    ///
    /// Updates are sent in this order:
    /// 1. [`ScanUpdate::PathsDiscovered`] - once, after collecting all paths
    /// 2. [`ScanUpdate::FileScanned`] or [`ScanUpdate::FileError`] - per file
    /// 3. [`ScanUpdate::Complete`] - once, after all files processed
    ///
    /// # Cancellation
    ///
    /// If the receiver is dropped, `blocking_send` will fail and rayon threads
    /// will exit cleanly. The scan will stop early but the method still returns `Ok`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use ch_scanner::{Scanner, ScanConfig, ScanUpdate};
    /// use tokio::sync::mpsc;
    ///
    /// let (tx, mut rx) = mpsc::channel(256);
    /// let scanner = Scanner::new(ScanConfig::new(Utf8Path::new("./src")))?;
    ///
    /// tokio::task::spawn_blocking(move || {
    ///     scanner.scan_streaming(tx).ok();
    /// });
    ///
    /// while let Some(update) = rx.recv().await {
    ///     // Process updates...
    /// }
    /// ```
    #[allow(clippy::needless_pass_by_value)] // Sender is cloned internally for rayon threads
    pub fn scan_streaming(&self, tx: mpsc::Sender<ScanUpdate>) -> Result<(), ScanError> {
        info!(root = %self.config.root, "Starting streaming scan");

        // Reset statistics for fresh scan
        self.stats.reset();
        self.cache.clear();

        // Walk directory to collect paths
        let walker = self.build_walker()?;
        let paths = walker.collect_paths()?;
        let path_count = paths.len();

        info!(count = path_count, "Collected TypeScript files");

        // Send paths discovered notification
        if tx.blocking_send(ScanUpdate::PathsDiscovered(path_count)).is_err() {
            // Receiver dropped, return early
            return Ok(());
        }

        // Determine registry reference for filtering
        let registry_ref = if self.config.use_registry {
            Some(self.registry.as_ref())
        } else {
            None
        };

        // Analyze files in parallel, streaming results
        let analyzer = FileAnalyzer::new();
        let errors = analyzer.analyze_files_streaming(
            &paths,
            &self.model_path_matcher,
            registry_ref,
            &tx,
            &self.cache,
            &self.stats,
        );

        // Build final result
        let stats = self.stats.snapshot();
        let result = ScanResult { stats, errors };

        info!(
            total = result.stats.total,
            legacy = result.stats.legacy,
            migrated = result.stats.migrated,
            partial = result.stats.partial,
            errors = result.stats.errors,
            "Streaming scan completed"
        );

        // Send completion notification (ignore if receiver dropped)
        let _ = tx.blocking_send(ScanUpdate::Complete(result));

        Ok(())
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

        // Determine registry reference for filtering
        let registry_ref = if self.config.use_registry {
            Some(self.registry.as_ref())
        } else {
            None
        };

        let analyzer = FileAnalyzer::new();
        let results = analyzer.analyze_files(paths, &self.model_path_matcher, registry_ref);

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
    pub fn cache(&self) -> &ScanCache {
        &self.cache
    }

    /// Returns the scanner configuration.
    #[must_use]
    pub const fn config(&self) -> &ScanConfig {
        &self.config
    }

    /// Returns a reference to the model registry.
    ///
    /// The registry contains all known model exports from the shared directories.
    /// Use this for model lookup or to display registry statistics in the TUI.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let registry = scanner.registry();
    /// println!("Legacy models: {}", registry.legacy_model_count());
    /// println!("Modern models: {}", registry.modern_model_count());
    ///
    /// // Check if a name is a known model export
    /// if registry.is_legacy_export("ActiveContractCodeGen") {
    ///     println!("Found legacy model export");
    /// }
    /// ```
    #[must_use]
    pub fn registry(&self) -> &ModelRegistry {
        &self.registry
    }

    /// Returns a clone of the Arc-wrapped registry for sharing across threads.
    #[must_use]
    pub fn registry_arc(&self) -> Arc<ModelRegistry> {
        Arc::clone(&self.registry)
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
        assert!(!config.use_registry);
        assert!(config.shared_path.is_none());
        assert!(config.shared_2023_path.is_none());
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
    fn test_scan_config_with_shared_paths() {
        let config = ScanConfig::new(Utf8Path::new("./src")).with_shared_paths(
            Utf8Path::new("./src/shared"),
            Utf8Path::new("./src/shared_2023"),
        );

        assert!(config.use_registry);
        assert_eq!(
            config.shared_path,
            Some(Utf8PathBuf::from("./src/shared"))
        );
        assert_eq!(
            config.shared_2023_path,
            Some(Utf8PathBuf::from("./src/shared_2023"))
        );
    }

    #[test]
    fn test_scan_config_with_registry() {
        let config = ScanConfig::new(Utf8Path::new("./src")).with_registry(true);
        assert!(config.use_registry);

        let config = ScanConfig::new(Utf8Path::new("./src")).with_registry(false);
        assert!(!config.use_registry);
    }

    #[test]
    fn test_scanner_invalid_root() {
        let config = ScanConfig::new(Utf8Path::new("/nonexistent/path/that/does/not/exist"));
        let result = Scanner::new(config);
        assert!(result.is_err());
    }
}
