//! UI components for the TUI.
//!
//! This module contains all the widget implementations for rendering
//! different parts of the interface.
//!
//! # Component Types
//!
//! - **Widgets** (`Widget` trait): Stateless rendering - `HeaderBar`, `StatsPanel`, `StatusBar`
//! - **Stateful Widgets** (`StatefulWidget` trait): Selection/scroll state - `FileListView`, `DetailPane`
//! - **Overlays**: Modal overlays - `HelpPanel`, `FilterInput`, `DirectoryInput`
//!
//! # Usage
//!
//! ```ignore
//! use ch_tui::components::{FileListView, HeaderBar};
//! ```

mod detail_pane;
mod directory_input;
mod file_list;
mod filter_input;
mod graph_summary;
mod header;
mod help;
mod model_drilldown;
mod stats_panel;
mod status_bar;

pub use detail_pane::DetailPane;
pub use directory_input::DirectoryInput;
pub use file_list::FileListView;
pub use filter_input::FilterInput;
pub use graph_summary::GraphSummaryPanel;
pub use header::HeaderBar;
pub use help::HelpPanel;
pub use model_drilldown::{ModelDrilldown, ModelListView};
pub use stats_panel::StatsPanel;
pub use status_bar::StatusBar;
