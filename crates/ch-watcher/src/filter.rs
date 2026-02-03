//! File filtering for watch events.
//!
//! This module provides traits and implementations for filtering file events
//! before they are sent to the event channel. Filtering at the source reduces
//! channel traffic and processing overhead in the async consumer.
//!
//! # Design
//!
//! The [`FileFilter`] trait defines a simple predicate for determining whether
//! a file event should be processed. Implementations can filter by:
//!
//! - File extension (e.g., only TypeScript files)
//! - Path patterns (e.g., exclude test files)
//! - Directory location (e.g., only watch certain directories)
//!
//! # Examples
//!
//! ```
//! use ch_watcher::{FileFilter, TypeScriptFilter};
//! use camino::Utf8Path;
//!
//! let filter = TypeScriptFilter::default();
//!
//! // TypeScript files pass
//! assert!(filter.should_process(Utf8Path::new("src/app.ts")));
//! assert!(filter.should_process(Utf8Path::new("src/App.tsx")));
//!
//! // Non-TypeScript files are filtered
//! assert!(!filter.should_process(Utf8Path::new("src/app.js")));
//! assert!(!filter.should_process(Utf8Path::new("styles.css")));
//! ```

use camino::Utf8Path;
use smallvec::SmallVec;

/// A filter for determining which file events to process.
///
/// Implementations of this trait are called for each file event detected
/// by the watcher. Events that return `false` from [`should_process`] are
/// discarded before being sent to the event channel.
///
/// # Thread Safety
///
/// Filters must be [`Send`] and [`Sync`] because they are used from the
/// blocking watcher thread. They must also be `'static` to be moved into
/// the spawned task.
///
/// # Examples
///
/// ```
/// use ch_watcher::FileFilter;
/// use camino::Utf8Path;
///
/// struct AllFilesFilter;
///
/// impl FileFilter for AllFilesFilter {
///     fn should_process(&self, _path: &Utf8Path) -> bool {
///         true // Accept all files
///     }
/// }
/// ```
///
/// [`should_process`]: FileFilter::should_process
pub trait FileFilter: Send + Sync + 'static {
    /// Returns `true` if the file at the given path should be processed.
    ///
    /// # Arguments
    ///
    /// * `path` - The path of the file that changed
    ///
    /// # Returns
    ///
    /// `true` if the event should be sent to the channel, `false` to discard it.
    fn should_process(&self, path: &Utf8Path) -> bool;
}

/// A filter that accepts all files.
///
/// This is useful when you want to process all file changes without filtering,
/// or as a default filter when no specific filtering is needed.
///
/// # Examples
///
/// ```
/// use ch_watcher::{FileFilter, AcceptAllFilter};
/// use camino::Utf8Path;
///
/// let filter = AcceptAllFilter;
/// assert!(filter.should_process(Utf8Path::new("anything.txt")));
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptAllFilter;

impl FileFilter for AcceptAllFilter {
    #[inline]
    fn should_process(&self, _path: &Utf8Path) -> bool {
        true
    }
}

/// A filter for TypeScript files (.ts and .tsx).
///
/// This is the primary filter used by the migration tool to focus on
/// TypeScript source files while ignoring other file types.
///
/// # Configuration
///
/// By default, the filter:
/// - Accepts `.ts` and `.tsx` files
/// - Excludes test files (`.spec.ts`, `.test.ts`, etc.)
/// - Excludes declaration files (`.d.ts`)
///
/// These behaviors can be customized using the builder methods.
///
/// # Examples
///
/// ```
/// use ch_watcher::{FileFilter, TypeScriptFilter};
/// use camino::Utf8Path;
///
/// // Default filter
/// let filter = TypeScriptFilter::default();
/// assert!(filter.should_process(Utf8Path::new("src/app.ts")));
/// assert!(!filter.should_process(Utf8Path::new("src/app.spec.ts")));
///
/// // Include test files
/// let with_tests = TypeScriptFilter::new().include_tests();
/// assert!(with_tests.should_process(Utf8Path::new("src/app.spec.ts")));
/// ```
#[derive(Debug, Clone)]
pub struct TypeScriptFilter {
    /// Accepted file extensions (without the leading dot).
    extensions: SmallVec<[&'static str; 4]>,

    /// Patterns to exclude from processing.
    exclude_patterns: SmallVec<[&'static str; 4]>,

    /// Whether to include test files.
    include_tests: bool,

    /// Whether to include declaration files (.d.ts).
    include_declarations: bool,
}

impl TypeScriptFilter {
    /// Creates a new TypeScript filter with default settings.
    ///
    /// Default settings:
    /// - Extensions: `.ts`, `.tsx`
    /// - Excludes: test files, spec files, declaration files
    #[must_use]
    pub fn new() -> Self {
        Self {
            extensions: SmallVec::from_slice(&["ts", "tsx"]),
            exclude_patterns: SmallVec::from_slice(&[".spec.", ".test.", "__tests__", "__mocks__"]),
            include_tests: false,
            include_declarations: false,
        }
    }

    /// Configures the filter to include test files.
    ///
    /// By default, files matching test patterns (`.spec.ts`, `.test.ts`, etc.)
    /// are excluded. Call this method to include them.
    #[must_use]
    pub fn include_tests(mut self) -> Self {
        self.include_tests = true;
        self
    }

    /// Configures the filter to include TypeScript declaration files.
    ///
    /// By default, `.d.ts` files are excluded. Call this method to include them.
    #[must_use]
    pub fn include_declarations(mut self) -> Self {
        self.include_declarations = true;
        self
    }

    /// Adds additional extensions to accept.
    ///
    /// # Arguments
    ///
    /// * `ext` - The extension to add (without the leading dot)
    #[must_use]
    pub fn with_extension(mut self, ext: &'static str) -> Self {
        if !self.extensions.contains(&ext) {
            self.extensions.push(ext);
        }
        self
    }

    /// Adds an additional exclusion pattern.
    ///
    /// Files whose paths contain this pattern will be excluded.
    ///
    /// # Arguments
    ///
    /// * `pattern` - The pattern to exclude
    #[must_use]
    pub fn exclude_pattern(mut self, pattern: &'static str) -> Self {
        if !self.exclude_patterns.contains(&pattern) {
            self.exclude_patterns.push(pattern);
        }
        self
    }

    /// Checks if the file has a TypeScript extension.
    fn has_typescript_extension(&self, path: &Utf8Path) -> bool {
        path.extension()
            .is_some_and(|ext| self.extensions.contains(&ext))
    }

    /// Checks if the file is a TypeScript declaration file.
    #[allow(clippy::unused_self)] // Consistency with other methods
    fn is_declaration_file(&self, path: &Utf8Path) -> bool {
        path.as_str().ends_with(".d.ts") || path.as_str().ends_with(".d.tsx")
    }

    /// Checks if the file matches any exclusion pattern.
    fn matches_exclusion_pattern(&self, path: &Utf8Path) -> bool {
        let path_str = path.as_str();
        self.exclude_patterns
            .iter()
            .any(|pattern| path_str.contains(pattern))
    }
}

impl Default for TypeScriptFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl FileFilter for TypeScriptFilter {
    fn should_process(&self, path: &Utf8Path) -> bool {
        // Must have a TypeScript extension
        if !self.has_typescript_extension(path) {
            return false;
        }

        // Check for declaration files
        if !self.include_declarations && self.is_declaration_file(path) {
            return false;
        }

        // Check for test file patterns
        if !self.include_tests && self.matches_exclusion_pattern(path) {
            return false;
        }

        true
    }
}

/// A filter based on file extensions.
///
/// A more generic filter that accepts files with any of the specified extensions.
///
/// # Examples
///
/// ```
/// use ch_watcher::{FileFilter, ExtensionFilter};
/// use camino::Utf8Path;
///
/// let filter = ExtensionFilter::new(&["ts", "tsx", "js", "jsx"]);
/// assert!(filter.should_process(Utf8Path::new("src/app.ts")));
/// assert!(filter.should_process(Utf8Path::new("src/app.js")));
/// assert!(!filter.should_process(Utf8Path::new("styles.css")));
/// ```
#[derive(Debug, Clone)]
pub struct ExtensionFilter {
    extensions: SmallVec<[String; 8]>,
}

impl ExtensionFilter {
    /// Creates a new extension filter.
    ///
    /// # Arguments
    ///
    /// * `extensions` - The extensions to accept (without the leading dot)
    #[must_use]
    pub fn new(extensions: &[&str]) -> Self {
        Self {
            extensions: extensions.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// Creates an extension filter from owned strings.
    #[must_use]
    pub fn from_owned(extensions: Vec<String>) -> Self {
        Self {
            extensions: extensions.into_iter().collect(),
        }
    }
}

impl FileFilter for ExtensionFilter {
    fn should_process(&self, path: &Utf8Path) -> bool {
        path.extension()
            .is_some_and(|ext| self.extensions.iter().any(|e| e == ext))
    }
}

/// A composite filter that combines multiple filters with AND logic.
///
/// All filters must return `true` for the file to be processed.
///
/// # Examples
///
/// ```
/// use ch_watcher::{FileFilter, TypeScriptFilter, CompositeFilter};
/// use camino::Utf8Path;
///
/// // Custom filter that excludes node_modules
/// struct NoNodeModules;
/// impl FileFilter for NoNodeModules {
///     fn should_process(&self, path: &Utf8Path) -> bool {
///         !path.as_str().contains("node_modules")
///     }
/// }
///
/// let filter = CompositeFilter::new()
///     .and(TypeScriptFilter::default())
///     .and(NoNodeModules);
///
/// assert!(filter.should_process(Utf8Path::new("src/app.ts")));
/// assert!(!filter.should_process(Utf8Path::new("node_modules/pkg/index.ts")));
/// ```
pub struct CompositeFilter {
    filters: Vec<Box<dyn FileFilter>>,
}

impl CompositeFilter {
    /// Creates a new empty composite filter.
    ///
    /// An empty composite filter accepts all files.
    #[must_use]
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Adds a filter to the composite.
    ///
    /// # Arguments
    ///
    /// * `filter` - The filter to add
    #[must_use]
    pub fn and<F: FileFilter>(mut self, filter: F) -> Self {
        self.filters.push(Box::new(filter));
        self
    }
}

impl Default for CompositeFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl FileFilter for CompositeFilter {
    fn should_process(&self, path: &Utf8Path) -> bool {
        self.filters.is_empty() || self.filters.iter().all(|f| f.should_process(path))
    }
}

// Implement FileFilter for boxed filters
impl<F: FileFilter + ?Sized> FileFilter for Box<F> {
    fn should_process(&self, path: &Utf8Path) -> bool {
        (**self).should_process(path)
    }
}

// Implement FileFilter for Arc-wrapped filters (useful for shared filters)
impl<F: FileFilter + ?Sized> FileFilter for std::sync::Arc<F> {
    fn should_process(&self, path: &Utf8Path) -> bool {
        (**self).should_process(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accept_all_filter() {
        let filter = AcceptAllFilter;
        assert!(filter.should_process(Utf8Path::new("anything.txt")));
        assert!(filter.should_process(Utf8Path::new("src/app.ts")));
        assert!(filter.should_process(Utf8Path::new("")));
    }

    #[test]
    fn test_typescript_filter_basic() {
        let filter = TypeScriptFilter::default();

        // Accepts TypeScript files
        assert!(filter.should_process(Utf8Path::new("src/app.ts")));
        assert!(filter.should_process(Utf8Path::new("src/App.tsx")));

        // Rejects non-TypeScript files
        assert!(!filter.should_process(Utf8Path::new("src/app.js")));
        assert!(!filter.should_process(Utf8Path::new("styles.css")));
        assert!(!filter.should_process(Utf8Path::new("README.md")));
    }

    #[test]
    fn test_typescript_filter_excludes_tests() {
        let filter = TypeScriptFilter::default();

        assert!(!filter.should_process(Utf8Path::new("src/app.spec.ts")));
        assert!(!filter.should_process(Utf8Path::new("src/app.test.ts")));
        assert!(!filter.should_process(Utf8Path::new("__tests__/app.ts")));
        assert!(!filter.should_process(Utf8Path::new("__mocks__/api.ts")));
    }

    #[test]
    fn test_typescript_filter_include_tests() {
        let filter = TypeScriptFilter::new().include_tests();

        assert!(filter.should_process(Utf8Path::new("src/app.spec.ts")));
        assert!(filter.should_process(Utf8Path::new("src/app.test.ts")));
        assert!(filter.should_process(Utf8Path::new("__tests__/app.ts")));
    }

    #[test]
    fn test_typescript_filter_excludes_declarations() {
        let filter = TypeScriptFilter::default();

        assert!(!filter.should_process(Utf8Path::new("src/types.d.ts")));
        assert!(!filter.should_process(Utf8Path::new("global.d.ts")));
    }

    #[test]
    fn test_typescript_filter_include_declarations() {
        let filter = TypeScriptFilter::new().include_declarations();

        assert!(filter.should_process(Utf8Path::new("src/types.d.ts")));
        assert!(filter.should_process(Utf8Path::new("global.d.ts")));
    }

    #[test]
    fn test_typescript_filter_custom_extension() {
        let filter = TypeScriptFilter::new().with_extension("mts");

        assert!(filter.should_process(Utf8Path::new("src/app.ts")));
        assert!(filter.should_process(Utf8Path::new("src/app.mts")));
    }

    #[test]
    fn test_typescript_filter_custom_exclusion() {
        let filter = TypeScriptFilter::new().exclude_pattern("generated");

        assert!(!filter.should_process(Utf8Path::new("src/generated/api.ts")));
        assert!(filter.should_process(Utf8Path::new("src/manual/api.ts")));
    }

    #[test]
    fn test_extension_filter() {
        let filter = ExtensionFilter::new(&["ts", "tsx", "js"]);

        assert!(filter.should_process(Utf8Path::new("src/app.ts")));
        assert!(filter.should_process(Utf8Path::new("src/app.tsx")));
        assert!(filter.should_process(Utf8Path::new("src/app.js")));
        assert!(!filter.should_process(Utf8Path::new("src/app.jsx")));
        assert!(!filter.should_process(Utf8Path::new("styles.css")));
    }

    #[test]
    fn test_composite_filter_empty() {
        let filter = CompositeFilter::new();
        assert!(filter.should_process(Utf8Path::new("anything")));
    }

    #[test]
    fn test_composite_filter_and() {
        struct NoNodeModules;
        impl FileFilter for NoNodeModules {
            fn should_process(&self, path: &Utf8Path) -> bool {
                !path.as_str().contains("node_modules")
            }
        }

        let filter = CompositeFilter::new()
            .and(TypeScriptFilter::default())
            .and(NoNodeModules);

        assert!(filter.should_process(Utf8Path::new("src/app.ts")));
        assert!(!filter.should_process(Utf8Path::new("node_modules/pkg/index.ts")));
        assert!(!filter.should_process(Utf8Path::new("src/app.js")));
    }

    #[test]
    fn test_boxed_filter() {
        let filter: Box<dyn FileFilter> = Box::new(TypeScriptFilter::default());
        assert!(filter.should_process(Utf8Path::new("src/app.ts")));
        assert!(!filter.should_process(Utf8Path::new("src/app.js")));
    }

    #[test]
    fn test_arc_filter() {
        let filter = std::sync::Arc::new(TypeScriptFilter::default());
        assert!(filter.should_process(Utf8Path::new("src/app.ts")));
        assert!(!filter.should_process(Utf8Path::new("src/app.js")));
    }
}
