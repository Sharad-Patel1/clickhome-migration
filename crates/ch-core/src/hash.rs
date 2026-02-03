//! Fast hash map and hash set type aliases.
//!
//! This module provides type aliases for [`FxHashMap`] and [`FxHashSet`] from the
//! `rustc-hash` crate. These use the Fx hash algorithm which is approximately 2x
//! faster than the standard library's `HashMap` and `HashSet` for string keys.
//!
//! # Why `FxHash`?
//!
//! The Fx hash function was originally developed for the Rust compiler (`rustc`).
//! It's optimized for:
//!
//! - String and byte slice keys (common in this codebase)
//! - Small to medium-sized hash tables
//! - Cases where denial-of-service resistance is not required (internal use only)
//!
//! # Examples
//!
//! ```
//! use ch_core::{FxHashMap, FxHashSet, fx_hash_map, fx_hash_set};
//!
//! // Using the type aliases directly
//! let mut map: FxHashMap<String, i32> = FxHashMap::default();
//! map.insert("key".to_owned(), 42);
//!
//! // Using the convenience constructors
//! let map: FxHashMap<&str, i32> = fx_hash_map();
//! let set: FxHashSet<&str> = fx_hash_set();
//! ```

/// A [`HashMap`](std::collections::HashMap) using the Fx hash algorithm.
///
/// This is faster than the standard library's `HashMap` for string keys
/// but does not provide denial-of-service resistance.
pub type FxHashMap<K, V> = rustc_hash::FxHashMap<K, V>;

/// A [`HashSet`](std::collections::HashSet) using the Fx hash algorithm.
///
/// This is faster than the standard library's `HashSet` for string keys
/// but does not provide denial-of-service resistance.
pub type FxHashSet<V> = rustc_hash::FxHashSet<V>;

/// The hasher used by [`FxHashMap`] and [`FxHashSet`].
pub type FxBuildHasher = rustc_hash::FxBuildHasher;

/// Creates a new empty [`FxHashMap`].
///
/// This is equivalent to `FxHashMap::default()` but can be more ergonomic
/// in some contexts due to type inference.
///
/// # Examples
///
/// ```
/// use ch_core::fx_hash_map;
///
/// let map: ch_core::FxHashMap<String, i32> = fx_hash_map();
/// assert!(map.is_empty());
/// ```
#[inline]
#[must_use]
pub fn fx_hash_map<K, V>() -> FxHashMap<K, V> {
    FxHashMap::default()
}

/// Creates a new empty [`FxHashSet`].
///
/// This is equivalent to `FxHashSet::default()` but can be more ergonomic
/// in some contexts due to type inference.
///
/// # Examples
///
/// ```
/// use ch_core::fx_hash_set;
///
/// let set: ch_core::FxHashSet<String> = fx_hash_set();
/// assert!(set.is_empty());
/// ```
#[inline]
#[must_use]
pub fn fx_hash_set<V>() -> FxHashSet<V> {
    FxHashSet::default()
}

/// Creates a new [`FxHashMap`] with the specified capacity.
///
/// The map will be able to hold at least `capacity` elements without
/// reallocating.
///
/// # Examples
///
/// ```
/// use ch_core::fx_hash_map_with_capacity;
///
/// let map: ch_core::FxHashMap<String, i32> = fx_hash_map_with_capacity(100);
/// assert!(map.capacity() >= 100);
/// ```
#[inline]
#[must_use]
pub fn fx_hash_map_with_capacity<K, V>(capacity: usize) -> FxHashMap<K, V> {
    FxHashMap::with_capacity_and_hasher(capacity, rustc_hash::FxBuildHasher)
}

/// Creates a new [`FxHashSet`] with the specified capacity.
///
/// The set will be able to hold at least `capacity` elements without
/// reallocating.
///
/// # Examples
///
/// ```
/// use ch_core::fx_hash_set_with_capacity;
///
/// let set: ch_core::FxHashSet<String> = fx_hash_set_with_capacity(100);
/// assert!(set.capacity() >= 100);
/// ```
#[inline]
#[must_use]
pub fn fx_hash_set_with_capacity<V>(capacity: usize) -> FxHashSet<V> {
    FxHashSet::with_capacity_and_hasher(capacity, rustc_hash::FxBuildHasher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fx_hash_map_operations() {
        let mut map: FxHashMap<&str, i32> = fx_hash_map();
        map.insert("one", 1);
        map.insert("two", 2);
        assert_eq!(map.get("one"), Some(&1));
        assert_eq!(map.get("two"), Some(&2));
        assert_eq!(map.get("three"), None);
    }

    #[test]
    fn test_fx_hash_set_operations() {
        let mut set: FxHashSet<&str> = fx_hash_set();
        set.insert("one");
        set.insert("two");
        assert!(set.contains("one"));
        assert!(set.contains("two"));
        assert!(!set.contains("three"));
    }

    #[test]
    fn test_fx_hash_map_with_capacity() {
        let map: FxHashMap<String, i32> = fx_hash_map_with_capacity(100);
        assert!(map.capacity() >= 100);
    }

    #[test]
    fn test_fx_hash_set_with_capacity() {
        let set: FxHashSet<String> = fx_hash_set_with_capacity(100);
        assert!(set.capacity() >= 100);
    }
}
