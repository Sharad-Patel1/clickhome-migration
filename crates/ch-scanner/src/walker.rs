//! Directory traversal for TypeScript files.
//!
//! This module provides [`FileWalker`], which uses the `ignore` crate to
//! efficiently walk directories while respecting `.gitignore` patterns.
//!
//! # Features
//!
//! - Respects `.gitignore` and `.ignore` patterns
//! - Filters for TypeScript files (`.ts`, `.tsx`)
//! - Skips hidden directories and files
//! - Converts paths to UTF-8 [`Utf8PathBuf`](camino::Utf8PathBuf)
//!
//! # Examples
//!
//! ```ignore
//! use ch_scanner::FileWalker;
//! use camino::Utf8Path;
//!
//! let walker = FileWalker::new(Utf8Path::new("/path/to/project"))?;
//! let paths = walker.collect_paths()?;
//!
//! for path in &paths {
//!     println!("Found: {path}");
//! }
//! ```

use camino::{Utf8Path, Utf8PathBuf};
use ignore::WalkBuilder;

use crate::error::ScanError;

/// Default directories to skip during scanning.
///
/// These directories typically don't contain TypeScript files that need
/// migration analysis or would be excluded anyway.
const SKIP_DIRECTORIES: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    ".git",
    ".angular",
    "coverage",
    "__pycache__",
    ".turbo",
    ".next",
    ".nuxt",
];

/// TypeScript file extensions to include in the scan.
const TYPESCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx"];

/// A file walker that discovers TypeScript files in a directory tree.
///
/// Uses the `ignore` crate for efficient traversal with gitignore support.
///
/// # Design
///
/// The walker uses a "collect-then-parallelize" pattern:
/// 1. Walker collects all paths first (single-threaded, I/O bound)
/// 2. Paths are then processed in parallel with rayon
///
/// This approach is memory-bounded and works well for enterprise codebases.
///
/// # Examples
///
/// ```ignore
/// use ch_scanner::FileWalker;
/// use camino::Utf8Path;
///
/// let walker = FileWalker::new(Utf8Path::new("./src"))?;
/// let paths = walker.collect_paths()?;
///
/// println!("Found {} TypeScript files", paths.len());
/// ```
#[derive(Debug)]
pub struct FileWalker {
    /// The root directory to walk.
    root: Utf8PathBuf,
    /// Additional directories to skip (beyond standard filters).
    skip_dirs: Vec<String>,
    /// Whether to follow symbolic links.
    follow_links: bool,
}

impl FileWalker {
    /// Creates a new file walker for the given root directory.
    ///
    /// # Arguments
    ///
    /// * `root` - The root directory to start walking from
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::Config`] if the root path doesn't exist or
    /// isn't a directory.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use ch_scanner::FileWalker;
    /// use camino::Utf8Path;
    ///
    /// let walker = FileWalker::new(Utf8Path::new("./src"))?;
    /// ```
    pub fn new(root: &Utf8Path) -> Result<Self, ScanError> {
        if !root.exists() {
            return Err(ScanError::config(format!(
                "root path does not exist: {root}"
            )));
        }
        if !root.is_dir() {
            return Err(ScanError::config(format!(
                "root path is not a directory: {root}"
            )));
        }

        Ok(Self {
            root: root.to_owned(),
            skip_dirs: Vec::new(),
            follow_links: false,
        })
    }

    /// Adds directories to skip during traversal.
    ///
    /// These are in addition to the default skip list (`node_modules`, `dist`, etc.).
    ///
    /// # Arguments
    ///
    /// * `dirs` - Directory names to skip (not full paths)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let walker = FileWalker::new(root)?
    ///     .with_skip_dirs(&["vendor", "third_party"]);
    /// ```
    #[must_use]
    pub fn with_skip_dirs(mut self, dirs: &[&str]) -> Self {
        self.skip_dirs.extend(dirs.iter().map(ToString::to_string));
        self
    }

    /// Configures whether to follow symbolic links.
    ///
    /// By default, symbolic links are not followed.
    ///
    /// # Arguments
    ///
    /// * `follow` - Whether to follow symbolic links
    #[must_use]
    pub const fn with_follow_links(mut self, follow: bool) -> Self {
        self.follow_links = follow;
        self
    }

    /// Collects all TypeScript file paths in the directory tree.
    ///
    /// Walks the directory tree starting from the root, filtering for
    /// TypeScript files (`.ts`, `.tsx`) and respecting gitignore patterns.
    ///
    /// # Returns
    ///
    /// A vector of UTF-8 paths to TypeScript files.
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::Walk`] if directory traversal fails.
    /// Returns [`ScanError::NonUtf8Path`] if a non-UTF-8 path is encountered.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let walker = FileWalker::new(root)?;
    /// let paths = walker.collect_paths()?;
    ///
    /// for path in &paths {
    ///     println!("Found TypeScript file: {path}");
    /// }
    /// ```
    pub fn collect_paths(&self) -> Result<Vec<Utf8PathBuf>, ScanError> {
        let mut paths = Vec::new();
        let walker = self.build_walker();

        for result in walker {
            let entry = result?;

            // Skip directories and non-files
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = entry.path();

            // Convert to UTF-8 path
            let utf8_path =
                Utf8Path::from_path(path).ok_or_else(|| ScanError::NonUtf8Path(path.to_owned()))?;

            // Check if it's a TypeScript file
            if !self.is_typescript_file(utf8_path) {
                continue;
            }

            // Skip files in excluded directories
            if self.should_skip_path(utf8_path) {
                continue;
            }

            paths.push(utf8_path.to_owned());
        }

        Ok(paths)
    }

    /// Builds the ignore walker with configured settings.
    fn build_walker(&self) -> ignore::Walk {
        WalkBuilder::new(&self.root)
            // Enable standard filters (.gitignore, .ignore, hidden files)
            .standard_filters(true)
            // Don't follow links by default
            .follow_links(self.follow_links)
            // Use a single thread for walking (we parallelize later)
            .threads(1)
            // Don't require the root to be a git repo
            .require_git(false)
            .build()
    }

    /// Checks if a path is a TypeScript file based on extension.
    #[allow(clippy::unused_self)] // Method signature kept for consistency
    fn is_typescript_file(&self, path: &Utf8Path) -> bool {
        path.extension()
            .is_some_and(|ext| TYPESCRIPT_EXTENSIONS.contains(&ext))
    }

    /// Checks if a path should be skipped based on directory name.
    fn should_skip_path(&self, path: &Utf8Path) -> bool {
        // Check each component of the path
        for component in path.components() {
            let component_str = component.as_str();

            // Skip standard directories
            if SKIP_DIRECTORIES.contains(&component_str) {
                return true;
            }

            // Skip user-specified directories
            if self.skip_dirs.iter().any(|d| d == component_str) {
                return true;
            }
        }

        false
    }

    /// Returns the root directory being walked.
    #[inline]
    #[must_use]
    pub fn root(&self) -> &Utf8Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_typescript_file() {
        let walker = FileWalker {
            root: Utf8PathBuf::from("."),
            skip_dirs: Vec::new(),
            follow_links: false,
        };

        assert!(walker.is_typescript_file(Utf8Path::new("foo.ts")));
        assert!(walker.is_typescript_file(Utf8Path::new("foo.tsx")));
        assert!(walker.is_typescript_file(Utf8Path::new("src/bar.ts")));
        assert!(!walker.is_typescript_file(Utf8Path::new("foo.js")));
        assert!(!walker.is_typescript_file(Utf8Path::new("foo.json")));
        assert!(!walker.is_typescript_file(Utf8Path::new("foo")));
    }

    #[test]
    fn test_should_skip_path() {
        let walker = FileWalker {
            root: Utf8PathBuf::from("."),
            skip_dirs: vec!["custom_skip".to_owned()],
            follow_links: false,
        };

        // Standard skip directories
        assert!(walker.should_skip_path(Utf8Path::new("node_modules/foo.ts")));
        assert!(walker.should_skip_path(Utf8Path::new("src/node_modules/bar.ts")));
        assert!(walker.should_skip_path(Utf8Path::new("dist/foo.ts")));
        assert!(walker.should_skip_path(Utf8Path::new(".git/hooks.ts")));

        // Custom skip directories
        assert!(walker.should_skip_path(Utf8Path::new("custom_skip/foo.ts")));
        assert!(walker.should_skip_path(Utf8Path::new("src/custom_skip/bar.ts")));

        // Should not skip
        assert!(!walker.should_skip_path(Utf8Path::new("src/foo.ts")));
        assert!(!walker.should_skip_path(Utf8Path::new("src/components/bar.ts")));
    }

    #[test]
    fn test_with_skip_dirs() {
        let walker = FileWalker {
            root: Utf8PathBuf::from("."),
            skip_dirs: Vec::new(),
            follow_links: false,
        }
        .with_skip_dirs(&["vendor", "third_party"]);

        assert!(walker.skip_dirs.contains(&"vendor".to_owned()));
        assert!(walker.skip_dirs.contains(&"third_party".to_owned()));
    }

    #[test]
    fn test_with_follow_links() {
        let walker = FileWalker {
            root: Utf8PathBuf::from("."),
            skip_dirs: Vec::new(),
            follow_links: false,
        }
        .with_follow_links(true);

        assert!(walker.follow_links);
    }
}
