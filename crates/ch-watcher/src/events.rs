//! Event types for file change notifications.
//!
//! This module provides types for representing file change events that are
//! emitted by the file watcher after debouncing.
//!
//! # Event Flow
//!
//! ```text
//! File System Change
//!        │
//!        ▼
//! notify-debouncer-mini (100ms debounce)
//!        │
//!        ▼
//!   FileEvent created
//!        │
//!        ▼
//!   Sent via channel to TUI
//! ```

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::time::Instant;

/// A file change event with a UTF-8 path guarantee.
///
/// Represents a single file that has changed, as detected by the file watcher
/// after debouncing. The event does not distinguish between create, modify, or
/// delete operations since the debouncer intentionally abstracts these details.
///
/// # Memory Efficiency
///
/// The path is stored as a [`Utf8PathBuf`] which guarantees UTF-8 encoding,
/// avoiding the need for error handling when displaying or processing paths.
///
/// # Examples
///
/// ```
/// use ch_watcher::FileEvent;
/// use camino::Utf8PathBuf;
/// use std::time::Instant;
///
/// let event = FileEvent::new(Utf8PathBuf::from("src/components/foo.ts"));
/// assert_eq!(event.path.as_str(), "src/components/foo.ts");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvent {
    /// The path of the file that changed.
    ///
    /// This is an absolute path to the changed file.
    pub path: Utf8PathBuf,

    /// The timestamp when this event was received.
    ///
    /// Uses [`Instant`] for monotonic timing, suitable for measuring
    /// elapsed time but not for wall-clock display.
    pub timestamp: Instant,
}

impl FileEvent {
    /// Creates a new file event for the given path.
    ///
    /// The timestamp is set to the current instant.
    ///
    /// # Arguments
    ///
    /// * `path` - The path of the file that changed
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_watcher::FileEvent;
    /// use camino::Utf8PathBuf;
    ///
    /// let event = FileEvent::new(Utf8PathBuf::from("src/app.ts"));
    /// assert!(!event.path.as_str().is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn new(path: Utf8PathBuf) -> Self {
        Self {
            path,
            timestamp: Instant::now(),
        }
    }

    /// Creates a new file event with a specific timestamp.
    ///
    /// Useful for testing or when reconstructing events.
    ///
    /// # Arguments
    ///
    /// * `path` - The path of the file that changed
    /// * `timestamp` - The timestamp for this event
    #[inline]
    #[must_use]
    pub const fn with_timestamp(path: Utf8PathBuf, timestamp: Instant) -> Self {
        Self { path, timestamp }
    }

    /// Returns the file extension, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_watcher::FileEvent;
    /// use camino::Utf8PathBuf;
    ///
    /// let event = FileEvent::new(Utf8PathBuf::from("src/app.ts"));
    /// assert_eq!(event.extension(), Some("ts"));
    ///
    /// let no_ext = FileEvent::new(Utf8PathBuf::from("Makefile"));
    /// assert_eq!(no_ext.extension(), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.path.extension()
    }

    /// Returns `true` if this is a TypeScript file (.ts or .tsx).
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_watcher::FileEvent;
    /// use camino::Utf8PathBuf;
    ///
    /// let ts_event = FileEvent::new(Utf8PathBuf::from("src/app.ts"));
    /// assert!(ts_event.is_typescript());
    ///
    /// let tsx_event = FileEvent::new(Utf8PathBuf::from("src/App.tsx"));
    /// assert!(tsx_event.is_typescript());
    ///
    /// let js_event = FileEvent::new(Utf8PathBuf::from("src/app.js"));
    /// assert!(!js_event.is_typescript());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_typescript(&self) -> bool {
        matches!(self.extension(), Some("ts" | "tsx"))
    }

    /// Returns the file name without the directory path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_watcher::FileEvent;
    /// use camino::Utf8PathBuf;
    ///
    /// let event = FileEvent::new(Utf8PathBuf::from("src/components/Button.tsx"));
    /// assert_eq!(event.file_name(), Some("Button.tsx"));
    /// ```
    #[inline]
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.path.file_name()
    }
}

/// A batch of file events received together.
///
/// Events may be batched when multiple files change within a short time window,
/// or when processing is slower than event arrival rate.
///
/// # Memory Efficiency
///
/// Uses [`SmallVec`] with inline storage for up to 8 events, avoiding heap
/// allocation in the common case of small batches.
///
/// # Examples
///
/// ```
/// use ch_watcher::{FileEvent, FileEventBatch};
/// use camino::Utf8PathBuf;
///
/// let mut batch = FileEventBatch::new();
/// batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts")));
/// batch.push(FileEvent::new(Utf8PathBuf::from("src/b.ts")));
///
/// assert_eq!(batch.len(), 2);
/// assert!(!batch.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct FileEventBatch {
    /// The events in this batch.
    pub events: SmallVec<[FileEvent; 8]>,

    /// The timestamp when this batch was created.
    pub received_at: Instant,
}

impl FileEventBatch {
    /// Creates a new empty batch.
    ///
    /// The `received_at` timestamp is set to the current instant.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: SmallVec::new(),
            received_at: Instant::now(),
        }
    }

    /// Creates a batch from a vector of events.
    ///
    /// # Arguments
    ///
    /// * `events` - The events to include in this batch
    #[inline]
    #[must_use]
    pub fn from_events(events: impl IntoIterator<Item = FileEvent>) -> Self {
        Self {
            events: events.into_iter().collect(),
            received_at: Instant::now(),
        }
    }

    /// Adds an event to the batch.
    #[inline]
    pub fn push(&mut self, event: FileEvent) {
        self.events.push(event);
    }

    /// Returns the number of events in this batch.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns `true` if the batch contains no events.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns an iterator over the events.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &FileEvent> {
        self.events.iter()
    }

    /// Returns an iterator over TypeScript file events only.
    #[inline]
    pub fn typescript_events(&self) -> impl Iterator<Item = &FileEvent> {
        self.events.iter().filter(|e| e.is_typescript())
    }

    /// Returns the unique paths in this batch.
    ///
    /// Useful when multiple events for the same file are batched together.
    #[must_use]
    pub fn unique_paths(&self) -> Vec<&Utf8PathBuf> {
        let mut paths: Vec<&Utf8PathBuf> = self.events.iter().map(|e| &e.path).collect();
        paths.sort();
        paths.dedup();
        paths
    }
}

impl Default for FileEventBatch {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoIterator for FileEventBatch {
    type Item = FileEvent;
    type IntoIter = smallvec::IntoIter<[FileEvent; 8]>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

impl<'a> IntoIterator for &'a FileEventBatch {
    type Item = &'a FileEvent;
    type IntoIter = std::slice::Iter<'a, FileEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.iter()
    }
}

impl FromIterator<FileEvent> for FileEventBatch {
    fn from_iter<T: IntoIterator<Item = FileEvent>>(iter: T) -> Self {
        Self::from_events(iter)
    }
}

/// Summary statistics for a batch of events.
///
/// Provides a quick overview of what types of files changed in a batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBatchStats {
    /// Total number of events in the batch.
    pub total_events: usize,

    /// Number of TypeScript file events.
    pub typescript_events: usize,

    /// Number of unique files affected.
    pub unique_files: usize,
}

impl EventBatchStats {
    /// Computes statistics for a batch of events.
    #[must_use]
    pub fn from_batch(batch: &FileEventBatch) -> Self {
        Self {
            total_events: batch.len(),
            typescript_events: batch.typescript_events().count(),
            unique_files: batch.unique_paths().len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_event_new() {
        let event = FileEvent::new(Utf8PathBuf::from("src/app.ts"));
        assert_eq!(event.path.as_str(), "src/app.ts");
    }

    #[test]
    fn test_file_event_extension() {
        let ts = FileEvent::new(Utf8PathBuf::from("src/app.ts"));
        assert_eq!(ts.extension(), Some("ts"));

        let tsx = FileEvent::new(Utf8PathBuf::from("src/App.tsx"));
        assert_eq!(tsx.extension(), Some("tsx"));

        let no_ext = FileEvent::new(Utf8PathBuf::from("Makefile"));
        assert_eq!(no_ext.extension(), None);
    }

    #[test]
    fn test_file_event_is_typescript() {
        let ts = FileEvent::new(Utf8PathBuf::from("src/app.ts"));
        assert!(ts.is_typescript());

        let tsx = FileEvent::new(Utf8PathBuf::from("src/App.tsx"));
        assert!(tsx.is_typescript());

        let js = FileEvent::new(Utf8PathBuf::from("src/app.js"));
        assert!(!js.is_typescript());

        let css = FileEvent::new(Utf8PathBuf::from("src/app.css"));
        assert!(!css.is_typescript());
    }

    #[test]
    fn test_file_event_file_name() {
        let event = FileEvent::new(Utf8PathBuf::from("src/components/Button.tsx"));
        assert_eq!(event.file_name(), Some("Button.tsx"));
    }

    #[test]
    fn test_file_event_batch_new() {
        let batch = FileEventBatch::new();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
    }

    #[test]
    fn test_file_event_batch_push() {
        let mut batch = FileEventBatch::new();
        batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts")));
        batch.push(FileEvent::new(Utf8PathBuf::from("src/b.tsx")));

        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_file_event_batch_from_events() {
        let events = vec![
            FileEvent::new(Utf8PathBuf::from("src/a.ts")),
            FileEvent::new(Utf8PathBuf::from("src/b.ts")),
        ];
        let batch = FileEventBatch::from_events(events);
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn test_file_event_batch_typescript_events() {
        let mut batch = FileEventBatch::new();
        batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts")));
        batch.push(FileEvent::new(Utf8PathBuf::from("src/b.js")));
        batch.push(FileEvent::new(Utf8PathBuf::from("src/c.tsx")));

        let ts_events: Vec<_> = batch.typescript_events().collect();
        assert_eq!(ts_events.len(), 2);
    }

    #[test]
    fn test_file_event_batch_unique_paths() {
        let mut batch = FileEventBatch::new();
        batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts")));
        batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts"))); // Duplicate
        batch.push(FileEvent::new(Utf8PathBuf::from("src/b.ts")));

        let unique = batch.unique_paths();
        assert_eq!(unique.len(), 2);
    }

    #[test]
    fn test_file_event_batch_iter() {
        let mut batch = FileEventBatch::new();
        batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts")));
        batch.push(FileEvent::new(Utf8PathBuf::from("src/b.ts")));

        let paths: Vec<_> = batch.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn test_file_event_batch_into_iter() {
        let mut batch = FileEventBatch::new();
        batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts")));
        batch.push(FileEvent::new(Utf8PathBuf::from("src/b.ts")));

        let events: Vec<_> = batch.into_iter().collect();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_file_event_batch_from_iterator() {
        let events = vec![
            FileEvent::new(Utf8PathBuf::from("src/a.ts")),
            FileEvent::new(Utf8PathBuf::from("src/b.ts")),
        ];
        let batch: FileEventBatch = events.into_iter().collect();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn test_event_batch_stats() {
        let mut batch = FileEventBatch::new();
        batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts")));
        batch.push(FileEvent::new(Utf8PathBuf::from("src/a.ts"))); // Duplicate
        batch.push(FileEvent::new(Utf8PathBuf::from("src/b.js")));

        let stats = EventBatchStats::from_batch(&batch);
        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.typescript_events, 2);
        assert_eq!(stats.unique_files, 2);
    }
}
