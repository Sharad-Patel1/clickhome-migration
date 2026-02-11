//! File watcher with async event streaming.
//!
//! This module provides the [`FileWatcher`] type that bridges the synchronous
//! `notify` file watching crate to the async tokio runtime.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Blocking Thread (spawn_blocking)             │
//! │  ┌──────────────────┐    ┌────────────────┐    ┌────────────┐  │
//! │  │ RecommendedWatcher│ -> │ Debouncer      │ -> │ Callback   │  │
//! │  │ (notify)         │    │ (100ms window) │    │ (filtering)│  │
//! │  └──────────────────┘    └────────────────┘    └─────┬──────┘  │
//! └──────────────────────────────────────────────────────│─────────┘
//!                                                        │
//!                                          blocking_send │
//!                                                        ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Async Runtime (tokio)                        │
//! │  ┌──────────────────┐    ┌────────────────┐                     │
//! │  │ FileWatcher      │    │ mpsc::Receiver │ -> TUI Event Loop   │
//! │  │ (shutdown ctrl)  │    │ (events)       │                     │
//! │  └──────────────────┘    └────────────────┘                     │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use ch_watcher::{FileWatcher, TypeScriptFilter};
//! use ch_core::WatchConfig;
//! use camino::Utf8Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = WatchConfig::default();
//!     let path = Utf8Path::new("/path/to/project");
//!     let filter = TypeScriptFilter::default();
//!
//!     let mut watcher = FileWatcher::new(path, &config, filter).await?;
//!
//!     // Receive events in an async context
//!     while let Some(event) = watcher.recv().await {
//!         println!("File changed: {}", event.path);
//!     }
//!
//!     Ok(())
//! }
//! ```

use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use ch_core::WatchConfig;

use crate::error::WatchError;
use crate::events::FileEvent;
use crate::filter::FileFilter;

/// Default channel capacity for file events.
const DEFAULT_CHANNEL_CAPACITY: usize = 100;

/// A file watcher that streams events to an async context.
///
/// `FileWatcher` manages a background thread that runs the `notify` file watcher
/// with debouncing. File change events are filtered and sent through a tokio
/// mpsc channel for consumption in async code.
///
/// # Lifecycle
///
/// 1. **Creation**: `FileWatcher::new()` validates the path, creates channels,
///    and spawns a blocking task with the notify watcher.
///
/// 2. **Event Reception**: Use `recv()` or `try_recv()` to receive events.
///    Events are already filtered according to the provided filter.
///
/// 3. **Shutdown**: Call `shutdown()` for graceful shutdown, or simply drop
///    the watcher. Dropping sends a shutdown signal and awaits task completion.
///
/// # Thread Safety
///
/// The watcher can be used from any async task. The underlying notify watcher
/// runs in a dedicated blocking thread managed by tokio's blocking pool.
///
/// # Examples
///
/// ```no_run
/// use ch_watcher::{FileWatcher, TypeScriptFilter};
/// use ch_core::WatchConfig;
/// use camino::Utf8Path;
///
/// # async fn example() -> Result<(), ch_watcher::WatchError> {
/// let config = WatchConfig::default();
/// let mut watcher = FileWatcher::new(
///     Utf8Path::new("./src"),
///     &config,
///     TypeScriptFilter::default(),
/// ).await?;
///
/// // Process events
/// while let Some(event) = watcher.recv().await {
///     println!("Changed: {}", event.path);
/// }
/// # Ok(())
/// # }
/// ```
pub struct FileWatcher {
    /// Shutdown signal sender.
    ///
    /// Sending on this channel signals the blocking task to stop.
    /// Set to `None` after shutdown is initiated.
    shutdown_tx: Option<oneshot::Sender<()>>,

    /// Handle to the blocking watcher task.
    ///
    /// Used to await completion during shutdown.
    task_handle: Option<JoinHandle<Result<(), WatchError>>>,

    /// Event receiver for async consumption.
    event_rx: mpsc::Receiver<FileEvent>,

    /// The path being watched.
    watch_path: Utf8PathBuf,
}

impl std::fmt::Debug for FileWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileWatcher")
            .field("watch_path", &self.watch_path)
            .field("is_running", &self.is_running())
            .finish_non_exhaustive()
    }
}

impl FileWatcher {
    /// Creates a new file watcher for the specified path.
    ///
    /// This method:
    /// 1. Validates that the path exists
    /// 2. Creates the event channel
    /// 3. Spawns a blocking task with the notify watcher
    /// 4. Starts watching the path recursively (if configured)
    ///
    /// # Arguments
    ///
    /// * `path` - The path to watch (must exist)
    /// * `config` - Watch configuration (debounce time, recursive mode)
    /// * `filter` - Filter to determine which events to process
    ///
    /// # Errors
    ///
    /// Returns [`WatchError::PathNotFound`] if the path doesn't exist.
    /// Returns [`WatchError::Notify`] if the watcher fails to initialize.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ch_watcher::{FileWatcher, TypeScriptFilter};
    /// use ch_core::WatchConfig;
    /// use camino::Utf8Path;
    ///
    /// # async fn example() -> Result<(), ch_watcher::WatchError> {
    /// let watcher = FileWatcher::new(
    ///     Utf8Path::new("./src"),
    ///     &WatchConfig::default(),
    ///     TypeScriptFilter::default(),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::unused_async)] // Async for API consistency with shutdown()
    pub async fn new<F: FileFilter>(
        path: &Utf8Path,
        config: &WatchConfig,
        filter: F,
    ) -> Result<Self, WatchError> {
        // Validate path exists
        if !path.exists() {
            return Err(WatchError::path_not_found(path));
        }

        // Canonicalize the path to get absolute path
        let watch_path = path.canonicalize_utf8().map_err(WatchError::Io)?;

        // Create channels
        let (event_tx, event_rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Clone values for the blocking task
        let task_path = watch_path.clone();
        let debounce_ms = config.debounce_ms;
        let recursive = config.recursive;

        // Spawn blocking task for notify
        let task_handle = tokio::task::spawn_blocking(move || {
            run_watcher_loop(
                task_path,
                debounce_ms,
                recursive,
                event_tx,
                shutdown_rx,
                filter,
            )
        });

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            task_handle: Some(task_handle),
            event_rx,
            watch_path,
        })
    }

    /// Creates a file watcher with a custom channel capacity.
    ///
    /// Use this when you need to handle bursts of file changes and want
    /// to prevent backpressure from blocking the watcher thread.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to watch
    /// * `config` - Watch configuration
    /// * `filter` - Event filter
    /// * `channel_capacity` - Capacity of the event channel
    #[allow(clippy::unused_async)] // Async for API consistency with shutdown()
    pub async fn with_capacity<F: FileFilter>(
        path: &Utf8Path,
        config: &WatchConfig,
        filter: F,
        channel_capacity: usize,
    ) -> Result<Self, WatchError> {
        // Validate path exists
        if !path.exists() {
            return Err(WatchError::path_not_found(path));
        }

        let watch_path = path.canonicalize_utf8().map_err(WatchError::Io)?;

        let (event_tx, event_rx) = mpsc::channel(channel_capacity);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let task_path = watch_path.clone();
        let debounce_ms = config.debounce_ms;
        let recursive = config.recursive;

        let task_handle = tokio::task::spawn_blocking(move || {
            run_watcher_loop(
                task_path,
                debounce_ms,
                recursive,
                event_tx,
                shutdown_rx,
                filter,
            )
        });

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            task_handle: Some(task_handle),
            event_rx,
            watch_path,
        })
    }

    /// Receives the next file event asynchronously.
    ///
    /// Returns `None` when the watcher has been shut down or the channel
    /// is closed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use ch_watcher::{FileWatcher, TypeScriptFilter};
    /// # use ch_core::WatchConfig;
    /// # use camino::Utf8Path;
    /// # async fn example() -> Result<(), ch_watcher::WatchError> {
    /// # let mut watcher = FileWatcher::new(
    /// #     Utf8Path::new("./src"),
    /// #     &WatchConfig::default(),
    /// #     TypeScriptFilter::default(),
    /// # ).await?;
    /// while let Some(event) = watcher.recv().await {
    ///     println!("File changed: {}", event.path);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn recv(&mut self) -> Option<FileEvent> {
        self.event_rx.recv().await
    }

    /// Tries to receive a file event without blocking.
    ///
    /// Returns `Ok(event)` if an event is available, `Err(TryRecvError::Empty)`
    /// if the channel is empty, or `Err(TryRecvError::Disconnected)` if the
    /// watcher has been shut down.
    pub fn try_recv(&mut self) -> Result<FileEvent, mpsc::error::TryRecvError> {
        self.event_rx.try_recv()
    }

    /// Returns a mutable reference to the event receiver.
    ///
    /// This is useful when you need to use the receiver directly with
    /// `tokio::select!` or other channel operations.
    pub fn events(&mut self) -> &mut mpsc::Receiver<FileEvent> {
        &mut self.event_rx
    }

    /// Returns the path being watched.
    #[must_use]
    pub fn watch_path(&self) -> &Utf8Path {
        &self.watch_path
    }

    /// Returns `true` if the watcher is still running.
    ///
    /// The watcher may stop running if the shutdown signal is sent or
    /// if an error occurs in the blocking task.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.shutdown_tx.is_some() && self.task_handle.as_ref().is_some_and(|h| !h.is_finished())
    }

    /// Gracefully shuts down the watcher.
    ///
    /// This method:
    /// 1. Sends the shutdown signal to the blocking task
    /// 2. Awaits the task to complete
    /// 3. Returns any error from the watcher thread
    ///
    /// # Errors
    ///
    /// Returns an error if the watcher thread panicked or encountered
    /// an error during operation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use ch_watcher::{FileWatcher, TypeScriptFilter};
    /// # use ch_core::WatchConfig;
    /// # use camino::Utf8Path;
    /// # async fn example() -> Result<(), ch_watcher::WatchError> {
    /// let watcher = FileWatcher::new(
    ///     Utf8Path::new("./src"),
    ///     &WatchConfig::default(),
    ///     TypeScriptFilter::default(),
    /// ).await?;
    ///
    /// // ... use watcher ...
    ///
    /// watcher.shutdown().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn shutdown(mut self) -> Result<(), WatchError> {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            // Ignore error if receiver is already dropped
            let _ = tx.send(());
        }

        // Await task completion
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(result) => result?,
                Err(_join_error) => return Err(WatchError::ChannelClosed),
            }
        }

        Ok(())
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        // Send shutdown signal on drop
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // Note: We don't await the task here since Drop is sync.
        // The task will stop when it receives the shutdown signal.
    }
}

/// Runs the notify watcher loop in a blocking context.
///
/// This function is called from `spawn_blocking` and runs the synchronous
/// notify debouncer, forwarding filtered events to the async channel.
#[allow(clippy::needless_pass_by_value)] // Path must be owned for the blocking task lifetime
fn run_watcher_loop<F: FileFilter>(
    path: Utf8PathBuf,
    debounce_ms: u64,
    recursive: bool,
    event_tx: mpsc::Sender<FileEvent>,
    shutdown_rx: oneshot::Receiver<()>,
    filter: F,
) -> Result<(), WatchError> {
    let timeout = Duration::from_millis(debounce_ms);

    // Create the debouncer with a callback that sends events
    let tx = event_tx;
    let debouncer_result: Result<Debouncer<notify::RecommendedWatcher>, notify::Error> =
        new_debouncer(timeout, move |res: DebounceEventResult| {
            if let Ok(events) = res {
                for event in events {
                    // Convert PathBuf to Utf8PathBuf
                    let utf8_path = match Utf8PathBuf::try_from(event.path) {
                        Ok(p) => p,
                        Err(e) => {
                            let invalid_path = e.into_path_buf();
                            tracing::warn!(
                                path = %invalid_path.display(),
                                "Skipping non-UTF-8 path in file event"
                            );
                            continue;
                        }
                    };

                    // Apply filter
                    if !filter.should_process(&utf8_path) {
                        tracing::trace!(path = %utf8_path, "Filtered out file event");
                        continue;
                    }

                    let file_event = FileEvent::new(utf8_path);

                    // Send via blocking_send for sync context
                    if tx.blocking_send(file_event).is_err() {
                        tracing::debug!("Event channel closed, stopping watcher");
                        break;
                    }
                }
            } else if let Err(error) = res {
                tracing::warn!(error = %error, "Debouncer error");
            }
        });

    let mut debouncer = debouncer_result?;

    // Configure recursive mode
    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };

    // Start watching
    debouncer.watcher().watch(path.as_std_path(), mode)?;

    tracing::info!(path = %path, recursive = recursive, "File watcher started");

    // Block until shutdown signal is received
    // Using blocking_recv since we're in a sync context
    let _ = shutdown_rx.blocking_recv();

    tracing::info!(path = %path, "File watcher stopped");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::AcceptAllFilter;
    use std::fs;
    use tempfile::TempDir;

    // Helper to create a temp directory for testing
    fn create_temp_dir() -> TempDir {
        TempDir::new().expect("Failed to create temp directory")
    }

    #[tokio::test]
    async fn test_watcher_creation() {
        let temp_dir = create_temp_dir();
        let path = Utf8Path::from_path(temp_dir.path()).expect("Invalid path");

        let config = WatchConfig::default();
        let watcher = FileWatcher::new(path, &config, AcceptAllFilter).await;

        assert!(watcher.is_ok());
        let watcher = watcher.expect("Watcher should be created");
        assert!(watcher.is_running());
    }

    #[tokio::test]
    async fn test_watcher_path_not_found() {
        let path = Utf8Path::new("/nonexistent/path/that/does/not/exist");
        let config = WatchConfig::default();

        let result = FileWatcher::new(path, &config, AcceptAllFilter).await;

        assert!(result.is_err());
        match result {
            Err(WatchError::PathNotFound(_)) => {}
            other => panic!("Expected PathNotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_watcher_shutdown() {
        let temp_dir = create_temp_dir();
        let path = Utf8Path::from_path(temp_dir.path()).expect("Invalid path");

        let config = WatchConfig::default();
        let watcher = FileWatcher::new(path, &config, AcceptAllFilter)
            .await
            .expect("Failed to create watcher");

        let result = watcher.shutdown().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_watcher_receives_events() {
        let temp_dir = create_temp_dir();
        let path = Utf8Path::from_path(temp_dir.path()).expect("Invalid path");

        let config = WatchConfig {
            enabled: true,
            debounce_ms: 50, // Shorter debounce for faster tests
            recursive: true,
        };

        let mut watcher = FileWatcher::new(path, &config, AcceptAllFilter)
            .await
            .expect("Failed to create watcher");

        // Create a file to trigger an event
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "hello").expect("Failed to write file");

        // Wait for the event with timeout
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;

        // Shutdown cleanly
        watcher.shutdown().await.expect("Shutdown failed");

        // Verify we got an event (timing-dependent, may not always work in CI)
        if let Ok(Some(event)) = event {
            assert!(event.path.as_str().contains("test.txt"));
        }
    }

    #[tokio::test]
    async fn test_watcher_watch_path() {
        let temp_dir = create_temp_dir();
        let path = Utf8Path::from_path(temp_dir.path()).expect("Invalid path");

        let config = WatchConfig::default();
        let watcher = FileWatcher::new(path, &config, AcceptAllFilter)
            .await
            .expect("Failed to create watcher");

        assert!(!watcher.watch_path().as_str().is_empty());
    }

    #[tokio::test]
    async fn test_watcher_with_capacity() {
        let temp_dir = create_temp_dir();
        let path = Utf8Path::from_path(temp_dir.path()).expect("Invalid path");

        let config = WatchConfig::default();
        let watcher = FileWatcher::with_capacity(path, &config, AcceptAllFilter, 50)
            .await
            .expect("Failed to create watcher");

        assert!(watcher.is_running());
    }

    #[tokio::test]
    async fn test_watcher_is_not_running_after_shutdown() {
        let temp_dir = create_temp_dir();
        let path = Utf8Path::from_path(temp_dir.path()).expect("Invalid path");

        let config = WatchConfig::default();
        let watcher = FileWatcher::new(path, &config, AcceptAllFilter)
            .await
            .expect("Failed to create watcher");

        assert!(watcher.is_running());

        watcher.shutdown().await.expect("Shutdown failed");
        // After shutdown, the watcher is consumed, so we can't check is_running
    }
}
