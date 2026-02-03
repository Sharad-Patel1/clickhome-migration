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
//! let results = analyzer.analyze_files(&paths);
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
use ch_core::{FileId, FileInfo, ImportInfo, MigrationStatus, ModelSource};
use ch_ts_parser::{detect_model_source_with, ArenaParser, ModelPathMatcher};
use rayon::prelude::*;
use rustc_hash::FxHasher;
use smallvec::SmallVec;

use crate::error::ScanError;

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
    ///
    /// # Returns
    ///
    /// A vector of `(path, Result<FileInfo, ScanError>)` tuples.
    /// Failed analyses return errors while successful ones continue.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let analyzer = FileAnalyzer::new();
    /// let results = analyzer.analyze_files(&paths);
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
                    );
                    (path.clone(), result)
                },
            )
            .collect()
    }

    /// Analyzes a single file.
    ///
    /// This is a convenience method for analyzing one file without parallel
    /// processing overhead. Creates its own parser instance.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to analyze
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

        for import in &mut imports {
            import.source = detect_model_source_with(&import.path, matcher);
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
