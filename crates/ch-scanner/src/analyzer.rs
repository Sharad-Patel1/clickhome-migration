//! Parallel file analysis using rayon and arena allocation.
//!
//! This module provides [`FileAnalyzer`], which orchestrates parallel parsing
//! of TypeScript files using `rayon` for parallelism and `bumpalo_herd` for
//! per-thread arena allocation.
//!
//! # Design
//!
//! Uses the "collect-then-parallelize" pattern:
//!
//! 1. Paths are collected first by [`FileWalker`](crate::FileWalker)
//! 2. `FileAnalyzer` processes paths in parallel with `rayon::par_iter()`
//! 3. Per-thread state (parser + arena) is initialized via `map_init()`
//! 4. Results are converted to owned data before arena scope ends
//!
//! # Registry-Based Filtering
//!
//! When a [`ModelRegistry`](ch_core::ModelRegistry) is provided, imports are filtered
//! to only include those that reference actual model exports. This eliminates false
//! positives from utility exports in the shared directories.
//!
//! # Performance
//!
//! - **Zero lock contention**: Per-thread arenas via `bumpalo_herd::Herd`
//! - **Efficient allocation**: Arena allocation for parse strings
//! - **Work stealing**: Rayon's work-stealing scheduler
//!
//! # Examples
//!
//! ```ignore
//! use ch_scanner::FileAnalyzer;
//! use camino::Utf8PathBuf;
//!
//! let analyzer = FileAnalyzer::new();
//! let paths: Vec<Utf8PathBuf> = vec![/* ... */];
//!
//! let results = analyzer.analyze_files(&paths, &matcher, None);
//!
//! for (path, result) in &results {
//!     match result {
//!         Ok(info) => println!("{}: {:?}", path, info.status),
//!         Err(e) => eprintln!("{}: {}", path, e),
//!     }
//! }
//! ```

use std::fs;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use bumpalo_herd::Herd;
use camino::{Utf8Path, Utf8PathBuf};
use ch_core::{FileId, FileInfo, ImportInfo, MigrationStatus, ModelRegistry, ModelSource};
use ch_ts_parser::{detect_model_source_with, ArenaParser, ModelPathMatcher};
use parking_lot::Mutex;
use rayon::prelude::*;
use rustc_hash::FxHasher;
use smallvec::SmallVec;
use tokio::sync::mpsc;

use crate::cache::ScanCache;
use crate::error::ScanError;
use crate::stats::ScanStats;
use crate::ScanUpdate;

/// Parallel file analyzer using rayon and per-thread arenas.
///
/// Processes TypeScript files in parallel, extracting imports and determining
/// migration status for each file.
///
/// # Thread Safety
///
/// `FileAnalyzer` is both `Send` and `Sync`. It creates per-thread parsers
/// and arenas during analysis, so no shared mutable state exists.
///
/// # Examples
///
/// ```ignore
/// use ch_scanner::FileAnalyzer;
///
/// let analyzer = FileAnalyzer::new();
/// let results = analyzer.analyze_files(&paths);
/// ```
#[derive(Debug, Default)]
pub struct FileAnalyzer {
    _private: (), // Prevent external construction
}

impl FileAnalyzer {
    /// Creates a new file analyzer.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use ch_scanner::FileAnalyzer;
    ///
    /// let analyzer = FileAnalyzer::new();
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Analyzes multiple files in parallel.
    ///
    /// Uses rayon's parallel iterator with per-thread parser and arena
    /// initialization. Each file is read, parsed, and analyzed independently.
    ///
    /// # Arguments
    ///
    /// * `paths` - Slice of file paths to analyze
    /// * `matcher` - Model path matcher for detecting shared directory imports
    /// * `registry` - Optional model registry for filtering imports to actual models
    ///
    /// # Returns
    ///
    /// A vector of `(path, Result<FileInfo, ScanError>)` tuples.
    /// Failed analyses return errors while successful ones continue.
    ///
    /// # Registry Filtering
    ///
    /// When a registry is provided, imports are validated against it:
    /// - Only imports where at least one imported name exists in the registry
    ///   are marked with a model source
    /// - This prevents false positives from utility exports in shared directories
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let analyzer = FileAnalyzer::new();
    /// let results = analyzer.analyze_files(&paths, &matcher, Some(&registry));
    ///
    /// let successful: Vec<_> = results
    ///     .into_iter()
    ///     .filter_map(|(p, r)| r.ok().map(|f| (p, f)))
    ///     .collect();
    /// ```
    #[must_use]
    pub fn analyze_files(
        &self,
        paths: &[Utf8PathBuf],
        matcher: &ModelPathMatcher,
        registry: Option<&ModelRegistry>,
    ) -> Vec<(Utf8PathBuf, Result<FileInfo, ScanError>)> {
        // Create a Herd for per-thread arenas
        let herd = Herd::new();

        paths
            .par_iter()
            .map_init(
                // Per-thread initialization: create parser + get arena member
                || {
                    let ts_parser = ArenaParser::new().ok();
                    let tsx_parser = ArenaParser::new_tsx().ok();
                    let member = herd.get();
                    (ts_parser, tsx_parser, member)
                },
                // Process each file
                |(ts_parser, tsx_parser, member), path| {
                    let result = self.analyze_file_inner(
                        path,
                        ts_parser.as_mut(),
                        tsx_parser.as_mut(),
                        member.as_bump(),
                        matcher,
                        registry,
                    );
                    (path.clone(), result)
                },
            )
            .collect()
    }

    /// Analyzes files in parallel, streaming results via channel.
    ///
    /// Unlike [`analyze_files`](Self::analyze_files), this method sends each
    /// result immediately via the channel rather than collecting them.
    /// This enables live UI updates during the scan.
    ///
    /// # Arguments
    ///
    /// * `paths` - Slice of file paths to analyze
    /// * `matcher` - Model path matcher for import detection
    /// * `registry` - Optional model registry for filtering imports
    /// * `tx` - Channel sender for streaming updates
    /// * `cache` - Cache to populate with successful results
    /// * `stats` - Statistics to update atomically
    ///
    /// # Returns
    ///
    /// A vector of errors encountered during scanning. Successful results
    /// are sent via the channel and inserted into the cache.
    ///
    /// # Cancellation
    ///
    /// If the channel receiver is dropped, `blocking_send` will fail and
    /// the remaining work will complete without sending updates.
    #[must_use]
    pub fn analyze_files_streaming(
        &self,
        paths: &[Utf8PathBuf],
        matcher: &ModelPathMatcher,
        registry: Option<&ModelRegistry>,
        tx: &mpsc::Sender<ScanUpdate>,
        cache: &ScanCache,
        stats: &ScanStats,
    ) -> Vec<(Utf8PathBuf, ScanError)> {
        // Create a Herd for per-thread arenas
        let herd = Herd::new();
        // Collect errors using mutex (errors are rare, so contention is minimal)
        let errors: Mutex<Vec<(Utf8PathBuf, ScanError)>> = Mutex::new(Vec::new());

        paths
            .par_iter()
            .for_each_init(
                // Per-thread initialization: create parser + get arena member
                || {
                    let ts_parser = ArenaParser::new().ok();
                    let tsx_parser = ArenaParser::new_tsx().ok();
                    let member = herd.get();
                    (ts_parser, tsx_parser, member, tx.clone())
                },
                // Process each file
                |(ts_parser, tsx_parser, member, sender), path| {
                    stats.increment_total();

                    let result = self.analyze_file_inner(
                        path,
                        ts_parser.as_mut(),
                        tsx_parser.as_mut(),
                        member.as_bump(),
                        matcher,
                        registry,
                    );

                    match result {
                        Ok(file_info) => {
                            // Update statistics based on status
                            match file_info.status {
                                MigrationStatus::Legacy => stats.increment_legacy(),
                                MigrationStatus::Migrated => stats.increment_migrated(),
                                MigrationStatus::Partial => stats.increment_partial(),
                                MigrationStatus::NoModels => stats.increment_no_models(),
                                _ => {} // Handle any future status variants
                            }

                            // Insert into cache
                            cache.insert(file_info.clone());

                            // Send update (ignore if receiver dropped)
                            // Box the FileInfo to match ScanUpdate::FileScanned(Box<FileInfo>)
                            let _ = sender.blocking_send(ScanUpdate::FileScanned(Box::new(file_info)));
                        }
                        Err(e) => {
                            stats.increment_errors();

                            // Collect error
                            errors.lock().push((path.clone(), e.clone()));

                            // Send error update (ignore if receiver dropped)
                            let _ = sender.blocking_send(ScanUpdate::FileError {
                                path: path.clone(),
                                error: e,
                            });
                        }
                    }
                },
            );

        // Return collected errors
        errors.into_inner()
    }

    /// Analyzes a single file.
    ///
    /// This is a convenience method for analyzing one file without parallel
    /// processing overhead. Creates its own parser instance.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to analyze
    /// * `matcher` - Model path matcher for detecting shared directory imports
    /// * `registry` - Optional model registry for filtering imports
    ///
    /// # Returns
    ///
    /// A [`FileInfo`] on success, or a [`ScanError`] on failure.
    ///
    /// # Errors
    ///
    /// - [`ScanError::Read`] if the file cannot be read
    /// - [`ScanError::Parse`] if the file cannot be parsed
    pub fn analyze_single(
        &self,
        path: &Utf8Path,
        matcher: &ModelPathMatcher,
        registry: Option<&ModelRegistry>,
    ) -> Result<FileInfo, ScanError> {
        let arena = bumpalo::Bump::new();
        let is_tsx = path.extension().is_some_and(|e| e == "tsx");

        let mut parser = if is_tsx {
            ArenaParser::new_tsx()
        } else {
            ArenaParser::new()
        }
        .map_err(|e| ScanError::parse(path, e))?;

        self.analyze_file_inner(
            path,
            Some(&mut parser),
            None,
            &arena,
            matcher,
            registry,
        )
    }

    /// Internal file analysis implementation.
    #[allow(clippy::unused_self)] // Method signature kept for consistency
    fn analyze_file_inner(
        &self,
        path: &Utf8Path,
        ts_parser: Option<&mut ArenaParser>,
        tsx_parser: Option<&mut ArenaParser>,
        arena: &bumpalo::Bump,
        matcher: &ModelPathMatcher,
        registry: Option<&ModelRegistry>,
    ) -> Result<FileInfo, ScanError> {
        // Read file contents
        let contents = fs::read_to_string(path.as_std_path())
            .map_err(|e| ScanError::read(path, e))?;

        // Calculate content hash
        let content_hash = hash_content(&contents);

        // Generate file ID from path hash
        let file_id = FileId::new(hash_path(path));

        // Select parser based on extension
        let is_tsx = path.extension().is_some_and(|e| e == "tsx");
        let parser = if is_tsx {
            tsx_parser.or(ts_parser)
        } else {
            ts_parser.or(tsx_parser)
        };

        let Some(parser) = parser else {
            return Err(ScanError::config("no parser available"));
        };

        // Parse the file
        let parse_result = parser
            .parse_with_arena(arena, &contents)
            .map_err(|e| ScanError::parse(path, e))?;

        // Convert imports to owned and calculate status
        let mut imports: SmallVec<[ImportInfo; 8]> = parse_result
            .imports
            .into_iter()
            .map(ch_ts_parser::BumpImportInfo::into_owned)
            .collect();

        // Process each import: detect source and optionally filter by registry
        for import in &mut imports {
            // First, detect if this is a shared directory import
            if let Some(detected_source) = detect_model_source_with(&import.path, matcher) {
                // If we have a registry, validate that at least one imported name
                // is a known model export from the detected source
                if let Some(reg) = registry {
                    let has_model_export = import.names.iter().any(|name| {
                        reg.is_export_from(name, detected_source)
                    });

                    // Only mark as model import if it has actual model exports
                    import.source = if has_model_export {
                        Some(detected_source)
                    } else {
                        None
                    };
                } else {
                    // No registry - use path-based detection only
                    import.source = Some(detected_source);
                }
            } else {
                import.source = None;
            }
        }

        let status = determine_status(&imports);

        // Get current timestamp
        let last_scanned = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(FileInfo {
            id: file_id,
            path: path.to_owned(),
            content_hash,
            imports,
            model_refs: SmallVec::new(), // TODO: populate from imports
            status,
            last_scanned,
        })
    }
}

/// Determines the migration status based on imports.
///
/// - legacy > 0 && new > 0: `Partial`
/// - legacy > 0 && new == 0: `Legacy`
/// - legacy == 0 && new > 0: `Migrated`
/// - legacy == 0 && new == 0: `NoModels`
fn determine_status(imports: &[ImportInfo]) -> MigrationStatus {
    let mut has_legacy = false;
    let mut has_new = false;

    for import in imports {
        match import.source {
            Some(ModelSource::SharedLegacy) => has_legacy = true,
            Some(ModelSource::Shared2023) => has_new = true,
            Some(_) | None => {} // Handle any future ModelSource variants or None
        }

        // Early exit if we've found both
        if has_legacy && has_new {
            return MigrationStatus::Partial;
        }
    }

    match (has_legacy, has_new) {
        (true, false) => MigrationStatus::Legacy,
        (false, true) => MigrationStatus::Migrated,
        (false, false) => MigrationStatus::NoModels,
        (true, true) => MigrationStatus::Partial, // Already handled above
    }
}

/// Computes a fast hash of file contents using `FxHash`.
fn hash_content(content: &str) -> u64 {
    let mut hasher = FxHasher::default();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Computes a fast hash of a file path using `FxHash`.
fn hash_path(path: &Utf8Path) -> u64 {
    let mut hasher = FxHasher::default();
    path.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_core::{ImportKind, SourceLocation};

    fn make_import(source: Option<ModelSource>) -> ImportInfo {
        ImportInfo::new(
            "test",
            ImportKind::Named,
            SmallVec::new(),
            source,
            SourceLocation::default(),
        )
    }

    #[test]
    fn test_determine_status_no_models() {
        let imports: Vec<ImportInfo> = vec![make_import(None), make_import(None)];
        assert_eq!(determine_status(&imports), MigrationStatus::NoModels);
    }

    #[test]
    fn test_determine_status_legacy() {
        let imports = vec![
            make_import(Some(ModelSource::SharedLegacy)),
            make_import(None),
        ];
        assert_eq!(determine_status(&imports), MigrationStatus::Legacy);
    }

    #[test]
    fn test_determine_status_migrated() {
        let imports = vec![
            make_import(Some(ModelSource::Shared2023)),
            make_import(None),
        ];
        assert_eq!(determine_status(&imports), MigrationStatus::Migrated);
    }

    #[test]
    fn test_determine_status_partial() {
        let imports = vec![
            make_import(Some(ModelSource::SharedLegacy)),
            make_import(Some(ModelSource::Shared2023)),
        ];
        assert_eq!(determine_status(&imports), MigrationStatus::Partial);
    }

    #[test]
    fn test_determine_status_empty() {
        let imports: Vec<ImportInfo> = vec![];
        assert_eq!(determine_status(&imports), MigrationStatus::NoModels);
    }

    #[test]
    fn test_hash_content_consistent() {
        let content = "test content";
        let hash1 = hash_content(content);
        let hash2 = hash_content(content);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_content_different() {
        let hash1 = hash_content("content 1");
        let hash2 = hash_content("content 2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_path_consistent() {
        let path = Utf8Path::new("src/foo.ts");
        let hash1 = hash_path(path);
        let hash2 = hash_path(path);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_path_different() {
        let hash1 = hash_path(Utf8Path::new("src/foo.ts"));
        let hash2 = hash_path(Utf8Path::new("src/bar.ts"));
        assert_ne!(hash1, hash2);
    }
}
