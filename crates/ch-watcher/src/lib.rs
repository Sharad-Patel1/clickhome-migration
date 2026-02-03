//! File watcher with debouncing and async event streaming.
//!
//! This crate provides:
//!
//! - File change detection via the `notify` crate
//! - Debouncing to batch rapid changes
//! - Async event stream using tokio channels
//! - Bridge between sync notify and async tokio via `spawn_blocking`

#![deny(clippy::all)]
#![warn(missing_docs)]

// TODO: Add modules during implementation
// pub mod watcher;
// pub mod events;
// pub mod debounce;
