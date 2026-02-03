//! File watcher with debouncing and async event streaming.
//!
//! This crate provides file change detection via the `notify` crate with
//! debouncing through `notify-debouncer-mini`, bridged to an async tokio
//! context for integration with the TUI event loop.
//!
//! # Overview
//!
//! The ch-watcher crate is designed to:
//!
//! - Detect file changes in the `ClickHome` codebase
//! - Debounce rapid changes (e.g., during save operations) with a 100ms window
//! - Filter events to focus on TypeScript files
//! - Stream events asynchronously to the TUI for live updates
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
//! # Crate Dependencies
//!
//! ```text
//! ch-cli ──► ch-tui ──► ch-scanner ──► ch-ts-parser ──► ch-core
//!                   ├─► ch-watcher ─────────────────────────►
//! ```
//!
//! # Usage
//!
//! ## Basic File Watching
//!
//! ```no_run
//! use ch_watcher::{FileWatcher, TypeScriptFilter};
//! use ch_core::WatchConfig;
//! use camino::Utf8Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Configure the watcher
//!     let config = WatchConfig::default(); // 100ms debounce, recursive
//!     let path = Utf8Path::new("/path/to/WebApp.Desktop/src");
//!     let filter = TypeScriptFilter::default();
//!
//!     // Create and start the watcher
//!     let mut watcher = FileWatcher::new(path, &config, filter).await?;
//!
//!     // Process events in an async loop
//!     while let Some(event) = watcher.recv().await {
//!         println!("File changed: {}", event.path);
//!         // Trigger re-analysis of the changed file
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Using with `tokio::select!`
//!
//! ```no_run
//! use ch_watcher::{FileWatcher, TypeScriptFilter};
//! use ch_core::WatchConfig;
//! use camino::Utf8Path;
//! use tokio::time::{interval, Duration};
//!
//! # async fn example() -> Result<(), ch_watcher::WatchError> {
//! let config = WatchConfig::default();
//! let mut watcher = FileWatcher::new(
//!     Utf8Path::new("./src"),
//!     &config,
//!     TypeScriptFilter::default(),
//! ).await?;
//!
//! let mut tick = interval(Duration::from_millis(250));
//!
//! loop {
//!     tokio::select! {
//!         Some(event) = watcher.recv() => {
//!             println!("File changed: {}", event.path);
//!         }
//!         _ = tick.tick() => {
//!             // Periodic UI refresh
//!         }
//!     }
//! }
//! # }
//! ```
//!
//! ## Custom Filtering
//!
//! ```
//! use ch_watcher::{FileFilter, CompositeFilter, TypeScriptFilter};
//! use camino::Utf8Path;
//!
//! // Create a filter that excludes node_modules
//! struct NoNodeModules;
//!
//! impl FileFilter for NoNodeModules {
//!     fn should_process(&self, path: &Utf8Path) -> bool {
//!         !path.as_str().contains("node_modules")
//!     }
//! }
//!
//! // Combine filters
//! let filter = CompositeFilter::new()
//!     .and(TypeScriptFilter::default())
//!     .and(NoNodeModules);
//!
//! // Use with FileWatcher::new(path, &config, filter)
//! ```
//!
//! # Error Handling
//!
//! The crate uses [`WatchError`] for all error cases:
//!
//! ```
//! use ch_watcher::WatchError;
//!
//! fn handle_watch_error(err: WatchError) {
//!     if err.is_fatal() {
//!         // Stop watching, show error to user
//!         eprintln!("Fatal watcher error: {}", err);
//!     } else {
//!         // Log and continue
//!         eprintln!("Warning: {}", err);
//!     }
//! }
//! ```
//!
//! # Performance Considerations
//!
//! - **Debouncing**: The 100ms debounce window batches rapid changes,
//!   preventing excessive updates during save operations.
//!
//! - **Filtering at Source**: Events are filtered in the blocking thread
//!   before being sent to the channel, reducing async processing overhead.
//!
//! - **Bounded Channel**: The event channel has a capacity of 100 events
//!   by default, preventing unbounded memory growth if the consumer is slow.
//!
//! - **UTF-8 Paths**: All paths are validated as UTF-8 early, avoiding
//!   repeated conversion overhead and ensuring consistent path handling.

#![deny(clippy::all)]
#![warn(missing_docs)]

pub mod error;
pub mod events;
pub mod filter;
pub mod watcher;

// Re-export error types
pub use error::WatchError;

// Re-export event types
pub use events::{EventBatchStats, FileEvent, FileEventBatch};

// Re-export filter types
pub use filter::{AcceptAllFilter, CompositeFilter, ExtensionFilter, FileFilter, TypeScriptFilter};

// Re-export watcher types
pub use watcher::FileWatcher;
