//! Arena-backed types for efficient parsing with zero-cost abstractions.
//!
//! This module provides arena-allocated versions of import types for use during
//! parsing. These types use borrowed strings from a [`bumpalo::Bump`] arena,
//! eliminating per-string allocations during the extraction phase.
//!
//! # Design
//!
//! The key insight is that during import extraction, we create many short-lived
//! strings (import paths, names) that are all freed at the same time when parsing
//! completes. By allocating these strings in a bump arena, we:
//!
//! - Reduce allocation count from O(imports * names) to O(1) arena grows
//! - Improve cache locality with contiguous memory
//! - Enable string interning for path deduplication
//!
//! # Usage
//!
//! For single-file parsing, use [`crate::TsParser`] which manages an internal arena.
//! For parallel scanning with rayon, use [`crate::ArenaParser`] with a
//! [`bumpalo_herd::Herd`] to get per-thread arenas.
//!
//! ```ignore
//! use bumpalo::Bump;
//! use ch_ts_parser::arena::{BumpImportInfo, StringInterner};
//!
//! let arena = Bump::new();
//! let mut interner = StringInterner::new(&arena);
//!
//! // Intern strings - repeated strings return same reference
//! let path1 = interner.intern("../shared/models/foo");
//! let path2 = interner.intern("../shared/models/foo");
//! assert!(std::ptr::eq(path1.as_str(), path2.as_str()));
//! ```

use bumpalo::Bump;
use ch_core::{FxHashMap, ImportInfo, ImportKind, ModelSource, SourceLocation};
use smallvec::SmallVec;
use std::hash::{Hash, Hasher};

/// Zero-cost newtype for arena-allocated strings.
///
/// Provides type safety for strings allocated in a [`Bump`] arena without
/// any runtime overhead. The newtype pattern ensures that arena-backed strings
/// are not accidentally mixed with owned strings.
///
/// # Zero-Cost Guarantee
///
/// This type has the same memory layout as `&str` and all methods are `#[inline]`,
/// so the compiler will optimize away the wrapper entirely.
///
/// # Examples
///
/// ```ignore
/// use bumpalo::Bump;
/// use ch_ts_parser::arena::ArenaStr;
///
/// let arena = Bump::new();
/// let s = ArenaStr::new(arena.alloc_str("hello"));
/// assert_eq!(s.as_str(), "hello");
/// assert_eq!(s.len(), 5);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ArenaStr<'a>(&'a str);

impl<'a> ArenaStr<'a> {
    /// Creates a new `ArenaStr` from an arena-allocated string slice.
    ///
    /// # Safety Note
    ///
    /// The caller must ensure that the string slice lives as long as the
    /// `ArenaStr`. This is automatically satisfied when using [`Bump::alloc_str`].
    #[inline]
    #[must_use]
    pub const fn new(s: &'a str) -> Self {
        Self(s)
    }

    /// Returns the underlying string slice.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }

    /// Returns the length of the string in bytes.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the string is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl AsRef<str> for ArenaStr<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl std::ops::Deref for ArenaStr<'_> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl PartialEq for ArenaStr<'_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for ArenaStr<'_> {}

impl Hash for ArenaStr<'_> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl std::fmt::Display for ArenaStr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Import information backed by arena allocation.
///
/// This is the arena-backed equivalent of [`ImportInfo`]. All string data
/// is borrowed from a [`Bump`] arena, eliminating per-string allocations.
///
/// Use [`From<BumpImportInfo<'_>>`] to convert to an owned [`ImportInfo`]
/// when the data needs to outlive the arena.
///
/// # Lifetime
///
/// The `'bump` lifetime is tied to the arena that holds the string data.
/// The `BumpImportInfo` must not outlive the arena.
///
/// # Examples
///
/// ```ignore
/// use bumpalo::Bump;
/// use ch_ts_parser::arena::{ArenaStr, BumpImportInfo};
/// use ch_core::{ImportInfo, ImportKind, SourceLocation};
/// use smallvec::smallvec;
///
/// let arena = Bump::new();
/// let bump_info = BumpImportInfo {
///     path: ArenaStr::new(arena.alloc_str("../shared/models/foo")),
///     kind: ImportKind::Named,
///     names: smallvec![ArenaStr::new(arena.alloc_str("Foo"))],
///     source: None,
///     location: SourceLocation::default(),
/// };
///
/// // Convert to owned when needed
/// let owned: ImportInfo = bump_info.into();
/// ```
#[derive(Debug, Clone)]
pub struct BumpImportInfo<'bump> {
    /// The module path from the import statement.
    pub path: ArenaStr<'bump>,

    /// The kind of import statement.
    pub kind: ImportKind,

    /// The names imported from the module.
    ///
    /// Uses `SmallVec` for stack allocation when there are 4 or fewer names.
    pub names: SmallVec<[ArenaStr<'bump>; 4]>,

    /// The detected model source, if from a shared directory.
    pub source: Option<ModelSource>,

    /// The location of the import statement in the source file.
    pub location: SourceLocation,
}

impl BumpImportInfo<'_> {
    /// Returns `true` if this import is from a shared model directory.
    #[inline]
    #[must_use]
    pub const fn is_model_import(&self) -> bool {
        self.source.is_some()
    }

    /// Returns `true` if this import is from the legacy shared directory.
    #[inline]
    #[must_use]
    pub fn is_legacy_import(&self) -> bool {
        self.source.is_some_and(ModelSource::is_legacy)
    }

    /// Converts this arena-backed import info into an owned [`ImportInfo`].
    ///
    /// This allocates new strings for the path and names.
    #[must_use]
    pub fn into_owned(self) -> ImportInfo {
        ImportInfo::new(
            self.path.as_str().to_owned(),
            self.kind,
            self.names.iter().map(|s| s.as_str().to_owned()).collect(),
            self.source,
            self.location,
        )
    }
}

impl From<BumpImportInfo<'_>> for ImportInfo {
    fn from(bump: BumpImportInfo<'_>) -> Self {
        bump.into_owned()
    }
}

/// Builder for constructing [`BumpImportInfo`] from captured tree-sitter nodes.
///
/// This is the arena-backed equivalent of `ImportBuilder`. It collects
/// information about an import statement during query iteration, then
/// builds the final [`BumpImportInfo`] when complete.
///
/// # Lifetime
///
/// The `'bump` lifetime is tied to the arena holding the string data,
/// and `'src` is tied to the source code being parsed.
#[derive(Debug)]
pub struct BumpImportBuilder<'bump> {
    /// Source path (the string after `from`).
    source_path: Option<ArenaStr<'bump>>,

    /// Imported names.
    names: SmallVec<[ArenaStr<'bump>; 4]>,

    /// The kind of import detected.
    kind: Option<ImportKind>,

    /// Source location of the import statement.
    location: SourceLocation,

    /// Whether this is a type-only import.
    is_type_only: bool,
}

impl<'bump> BumpImportBuilder<'bump> {
    /// Creates a new import builder with the given location and type-only flag.
    #[inline]
    #[must_use]
    pub fn new(location: SourceLocation, is_type_only: bool) -> Self {
        Self {
            source_path: None,
            names: SmallVec::new(),
            kind: None,
            location,
            is_type_only,
        }
    }

    /// Sets the source path.
    #[inline]
    pub fn set_source(&mut self, path: ArenaStr<'bump>) {
        self.source_path = Some(path);
    }

    /// Adds a named import identifier.
    #[inline]
    pub fn add_named_import(&mut self, name: ArenaStr<'bump>) {
        self.names.push(name);
        if self.kind.is_none() {
            self.kind = Some(ImportKind::Named);
        }
    }

    /// Sets this as a default import.
    #[inline]
    pub fn set_default_import(&mut self, name: ArenaStr<'bump>) {
        self.names.push(name);
        self.kind = Some(ImportKind::Default);
    }

    /// Sets this as a namespace import.
    #[inline]
    pub fn set_namespace_import(&mut self, name: ArenaStr<'bump>) {
        self.names.push(name);
        self.kind = Some(ImportKind::Namespace);
    }

    /// Builds the final [`BumpImportInfo`], returning `None` if incomplete.
    ///
    /// # Arguments
    ///
    /// * `detect_source` - Function to detect the model source from the path
    #[must_use]
    pub fn build<F>(self, detect_source: F) -> Option<BumpImportInfo<'bump>>
    where
        F: FnOnce(&str) -> Option<ModelSource>,
    {
        let path = self.source_path?;
        let source = detect_source(path.as_str());

        let kind = if self.is_type_only {
            ImportKind::TypeOnly
        } else if let Some(k) = self.kind {
            k
        } else if self.names.is_empty() {
            ImportKind::SideEffect
        } else {
            ImportKind::Named
        };

        Some(BumpImportInfo {
            path,
            kind,
            names: self.names,
            source,
            location: self.location,
        })
    }
}

/// Simple string interner backed by a bump arena.
///
/// Deduplicates strings within a parse session by storing them in a hash map.
/// Repeated strings return the same arena-allocated reference, saving memory
/// and improving cache locality.
///
/// # Performance
///
/// - Interning: O(1) average case (hash lookup)
/// - Memory: One allocation per unique string
/// - Common case: Import paths like `../shared/models/foo` appear many times
///
/// # Examples
///
/// ```ignore
/// use bumpalo::Bump;
/// use ch_ts_parser::arena::StringInterner;
///
/// let arena = Bump::new();
/// let mut interner = StringInterner::new(&arena);
///
/// let s1 = interner.intern("hello");
/// let s2 = interner.intern("hello");
///
/// // Same pointer - deduplicated
/// assert!(std::ptr::eq(s1.as_str(), s2.as_str()));
/// ```
#[derive(Debug)]
pub struct StringInterner<'bump> {
    arena: &'bump Bump,
    interned: FxHashMap<&'bump str, ArenaStr<'bump>>,
}

impl<'bump> StringInterner<'bump> {
    /// Creates a new string interner backed by the given arena.
    #[inline]
    #[must_use]
    pub fn new(arena: &'bump Bump) -> Self {
        Self {
            arena,
            interned: FxHashMap::default(),
        }
    }

    /// Creates a new string interner with pre-allocated capacity.
    ///
    /// Use this when you have an estimate of how many unique strings
    /// will be interned.
    #[inline]
    #[must_use]
    pub fn with_capacity(arena: &'bump Bump, capacity: usize) -> Self {
        Self {
            arena,
            interned: FxHashMap::with_capacity_and_hasher(capacity, ch_core::FxBuildHasher::default()),
        }
    }

    /// Interns a string, returning an arena-allocated reference.
    ///
    /// If the string has been interned before, returns the existing
    /// reference. Otherwise, allocates the string in the arena and
    /// stores it for future lookups.
    #[inline]
    pub fn intern(&mut self, s: &str) -> ArenaStr<'bump> {
        if let Some(&existing) = self.interned.get(s) {
            return existing;
        }

        let allocated = self.arena.alloc_str(s);
        let arena_str = ArenaStr::new(allocated);
        self.interned.insert(allocated, arena_str);
        arena_str
    }

    /// Returns the number of unique strings interned.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.interned.len()
    }

    /// Returns `true` if no strings have been interned.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.interned.is_empty()
    }

    /// Returns a reference to the underlying arena.
    #[inline]
    #[must_use]
    pub const fn arena(&self) -> &'bump Bump {
        self.arena
    }
}

/// Creates a dynamic import info directly from arena-allocated path.
///
/// This is a convenience function for creating dynamic import entries
/// without going through the builder pattern.
#[inline]
#[must_use]
pub fn create_dynamic_bump_import(
    path: ArenaStr<'_>,
    source: Option<ModelSource>,
    location: SourceLocation,
) -> BumpImportInfo<'_> {
    BumpImportInfo {
        path,
        kind: ImportKind::Dynamic,
        names: SmallVec::new(),
        source,
        location,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn test_arena_str_basic() {
        let arena = Bump::new();
        let s = ArenaStr::new(arena.alloc_str("hello"));

        assert_eq!(s.as_str(), "hello");
        assert_eq!(s.len(), 5);
        assert!(!s.is_empty());
    }

    #[test]
    fn test_arena_str_empty() {
        let arena = Bump::new();
        let s = ArenaStr::new(arena.alloc_str(""));

        assert_eq!(s.as_str(), "");
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
    }

    #[test]
    fn test_arena_str_equality() {
        let arena = Bump::new();
        let s1 = ArenaStr::new(arena.alloc_str("hello"));
        let s2 = ArenaStr::new(arena.alloc_str("hello"));
        let s3 = ArenaStr::new(arena.alloc_str("world"));

        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn test_arena_str_deref() {
        let arena = Bump::new();
        let s = ArenaStr::new(arena.alloc_str("hello world"));

        // Test Deref - can use str methods directly
        assert!(s.starts_with("hello"));
        assert!(s.ends_with("world"));
        assert!(s.contains(" "));
    }

    #[test]
    fn test_bump_import_info_into_owned() {
        let arena = Bump::new();
        let bump_info = BumpImportInfo {
            path: ArenaStr::new(arena.alloc_str("../shared/models/foo")),
            kind: ImportKind::Named,
            names: smallvec![
                ArenaStr::new(arena.alloc_str("Foo")),
                ArenaStr::new(arena.alloc_str("Bar")),
            ],
            source: Some(ModelSource::SharedLegacy),
            location: SourceLocation::new(10, 5, 245),
        };

        let owned: ImportInfo = bump_info.into();

        assert_eq!(owned.path, "../shared/models/foo");
        assert_eq!(owned.kind, ImportKind::Named);
        assert_eq!(owned.names.len(), 2);
        assert_eq!(owned.names[0], "Foo");
        assert_eq!(owned.names[1], "Bar");
        assert_eq!(owned.source, Some(ModelSource::SharedLegacy));
        assert_eq!(owned.location.line, 10);
    }

    #[test]
    fn test_bump_import_info_is_legacy() {
        let arena = Bump::new();

        let legacy = BumpImportInfo {
            path: ArenaStr::new(arena.alloc_str("../shared/models/foo")),
            kind: ImportKind::Named,
            names: smallvec![],
            source: Some(ModelSource::SharedLegacy),
            location: SourceLocation::default(),
        };
        assert!(legacy.is_legacy_import());
        assert!(legacy.is_model_import());

        let new = BumpImportInfo {
            path: ArenaStr::new(arena.alloc_str("../shared_2023/models/foo")),
            kind: ImportKind::Named,
            names: smallvec![],
            source: Some(ModelSource::Shared2023),
            location: SourceLocation::default(),
        };
        assert!(!new.is_legacy_import());
        assert!(new.is_model_import());

        let other = BumpImportInfo {
            path: ArenaStr::new(arena.alloc_str("@angular/core")),
            kind: ImportKind::Named,
            names: smallvec![],
            source: None,
            location: SourceLocation::default(),
        };
        assert!(!other.is_legacy_import());
        assert!(!other.is_model_import());
    }

    #[test]
    fn test_bump_import_builder_named() {
        let arena = Bump::new();
        let mut builder = BumpImportBuilder::new(SourceLocation::new(1, 0, 0), false);

        builder.set_source(ArenaStr::new(arena.alloc_str("'../shared/models/foo'")));
        builder.add_named_import(ArenaStr::new(arena.alloc_str("Foo")));
        builder.add_named_import(ArenaStr::new(arena.alloc_str("Bar")));

        let info = builder.build(|_| Some(ModelSource::SharedLegacy));
        assert!(info.is_some());

        let info = info.expect("should build");
        assert_eq!(info.kind, ImportKind::Named);
        assert_eq!(info.names.len(), 2);
    }

    #[test]
    fn test_bump_import_builder_default() {
        let arena = Bump::new();
        let mut builder = BumpImportBuilder::new(SourceLocation::default(), false);

        builder.set_source(ArenaStr::new(arena.alloc_str("'./foo'")));
        builder.set_default_import(ArenaStr::new(arena.alloc_str("Foo")));

        let info = builder.build(|_| None).expect("should build");
        assert_eq!(info.kind, ImportKind::Default);
        assert_eq!(info.names.len(), 1);
    }

    #[test]
    fn test_bump_import_builder_namespace() {
        let arena = Bump::new();
        let mut builder = BumpImportBuilder::new(SourceLocation::default(), false);

        builder.set_source(ArenaStr::new(arena.alloc_str("'./foo'")));
        builder.set_namespace_import(ArenaStr::new(arena.alloc_str("Foo")));

        let info = builder.build(|_| None).expect("should build");
        assert_eq!(info.kind, ImportKind::Namespace);
    }

    #[test]
    fn test_bump_import_builder_side_effect() {
        let arena = Bump::new();
        let mut builder = BumpImportBuilder::new(SourceLocation::default(), false);

        builder.set_source(ArenaStr::new(arena.alloc_str("'./polyfills'")));

        let info = builder.build(|_| None).expect("should build");
        assert_eq!(info.kind, ImportKind::SideEffect);
        assert!(info.names.is_empty());
    }

    #[test]
    fn test_bump_import_builder_type_only() {
        let arena = Bump::new();
        let mut builder = BumpImportBuilder::new(SourceLocation::default(), true);

        builder.set_source(ArenaStr::new(arena.alloc_str("'./types'")));
        builder.add_named_import(ArenaStr::new(arena.alloc_str("MyType")));

        let info = builder.build(|_| None).expect("should build");
        assert_eq!(info.kind, ImportKind::TypeOnly);
    }

    #[test]
    fn test_bump_import_builder_incomplete() {
        let builder = BumpImportBuilder::new(SourceLocation::default(), false);
        // No source path set - should return None
        let info = builder.build(|_| None);
        assert!(info.is_none());
    }

    #[test]
    fn test_string_interner_basic() {
        let arena = Bump::new();
        let mut interner = StringInterner::new(&arena);

        let s1 = interner.intern("hello");
        let s2 = interner.intern("world");
        let s3 = interner.intern("hello"); // Duplicate

        assert_eq!(s1.as_str(), "hello");
        assert_eq!(s2.as_str(), "world");
        assert_eq!(s3.as_str(), "hello");

        // s1 and s3 should be the same pointer (deduplicated)
        assert!(std::ptr::eq(s1.as_str(), s3.as_str()));

        // s1 and s2 should be different pointers
        assert!(!std::ptr::eq(s1.as_str(), s2.as_str()));

        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn test_string_interner_empty() {
        let arena = Bump::new();
        let interner = StringInterner::new(&arena);

        assert!(interner.is_empty());
        assert_eq!(interner.len(), 0);
    }

    #[test]
    fn test_string_interner_with_capacity() {
        let arena = Bump::new();
        let mut interner = StringInterner::with_capacity(&arena, 100);

        interner.intern("test");
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_create_dynamic_bump_import() {
        let arena = Bump::new();
        let path = ArenaStr::new(arena.alloc_str("'../shared/models/foo'"));

        let import = create_dynamic_bump_import(
            path,
            Some(ModelSource::SharedLegacy),
            SourceLocation::new(5, 10, 100),
        );

        assert_eq!(import.kind, ImportKind::Dynamic);
        assert!(import.names.is_empty());
        assert!(import.is_legacy_import());
        assert_eq!(import.location.line, 5);
    }

    #[test]
    fn test_arena_str_hash() {
        use std::collections::hash_map::DefaultHasher;

        let arena = Bump::new();
        let s1 = ArenaStr::new(arena.alloc_str("hello"));
        let s2 = ArenaStr::new(arena.alloc_str("hello"));

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        s1.hash(&mut hasher1);
        s2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
    }
}
