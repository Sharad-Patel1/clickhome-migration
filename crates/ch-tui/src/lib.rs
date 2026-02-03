//! Terminal user interface components using Ratatui.
//!
//! This crate provides a production-quality TUI for the ch-migration tool,
//! featuring an async event loop with tokio, file watcher integration,
//! stateful widgets with selection/scroll, and a component-based architecture.
//!
//! # Architecture
//!
//! ```text
//! crates/ch-tui/src/
//!   lib.rs           # Public API exports
//!   app.rs           # Application state and lifecycle
//!   event.rs         # Event types (Key, Mouse, File, Tick, Render)
//!   tui.rs           # Terminal wrapper with async event streaming
//!   action.rs        # User actions (commands from key bindings)
//!   ui.rs            # Main layout rendering orchestration
//!   theme.rs         # Color scheme and styling constants
//!   error.rs         # TUI-specific error types
//!   components/
//!     mod.rs         # Component trait definition
//!     file_list.rs   # FileListView + FileListState
//!     detail_pane.rs # DetailPane for selected file
//!     stats_panel.rs # StatsPanel with progress
//!     header.rs      # HeaderBar component
//!     status_bar.rs  # StatusBar component
//!     help.rs        # HelpPanel modal overlay
//!     filter_input.rs # Filter/search input component
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use ch_tui::{App, Tui, run};
//! use ch_core::Config;
//! use ch_scanner::Scanner;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), ch_tui::TuiError> {
//!     let config = Config::default();
//!     let scanner = Scanner::new(config.scan.clone().into())?;
//!     
//!     run(config, scanner).await
//! }
//! ```

#![deny(clippy::all)]
#![warn(missing_docs)]

pub mod action;
pub mod app;
pub mod components;
pub mod error;
pub mod event;
pub mod theme;
pub mod tui;
pub mod ui;

use ch_core::Config;
use ch_scanner::Scanner;
use ch_watcher::{FileWatcher, TypeScriptFilter};
use tracing::{debug, error, info};

// Public re-exports
pub use action::Action;
pub use app::{App, AppMode, DetailPaneState, FileListState, FilterState, Focus, StatusMessage};
pub use error::TuiError;
pub use event::Event;
pub use theme::Theme;
pub use tui::Tui;

/// Runs the TUI application with the given configuration and scanner.
///
/// This is the main entry point for the ch-tui crate. It:
///
/// 1. Initializes the terminal
/// 2. Performs an initial scan
/// 3. Optionally starts the file watcher
/// 4. Runs the main event loop
/// 5. Cleans up on exit
///
/// # Arguments
///
/// * `config` - The application configuration
/// * `scanner` - The file scanner (pre-configured)
///
/// # Errors
///
/// Returns an error if:
/// - Terminal initialization fails
/// - Initial scan fails
/// - File watcher fails to start
///
/// # Examples
///
/// ```ignore
/// use ch_tui::run;
/// use ch_core::Config;
/// use ch_scanner::Scanner;
///
/// #[tokio::main]
/// async fn main() -> Result<(), ch_tui::TuiError> {
///     let config = Config::default();
///     let scanner = Scanner::new(config.scan.clone().into())?;
///     run(config, scanner).await
/// }
/// ```
pub async fn run(config: Config, scanner: Scanner) -> Result<(), TuiError> {
    // Initialize TUI
    // tick_rate_ms and frame_rate are small UI timing values, precision loss is acceptable
    #[allow(clippy::cast_precision_loss)]
    let tick_rate = config.tui.tick_rate_ms as f64 / 1000.0;
    #[allow(clippy::cast_precision_loss)]
    let frame_rate = config.tui.frame_rate as f64;

    let mut tui = Tui::new(tick_rate)?.with_frame_rate(frame_rate);

    // Initialize app
    let mut app = App::new(config.clone(), scanner);

    let mut watcher = if app.needs_directory_setup() {
        debug!("Directory setup required; delaying initial scan and watcher");
        None
    } else {
        // Perform initial scan
        info!("Starting initial scan");
        app.initial_scan()?;

        // Start file watcher if enabled
        if config.watch.enabled {
            info!(path = %config.scan.root_path, "Starting file watcher");
            match FileWatcher::new(
                &config.scan.root_path,
                &config.watch,
                TypeScriptFilter::default(),
            )
            .await
            {
                Ok(w) => Some(w),
                Err(e) => {
                    error!(error = %e, "Failed to start file watcher");
                    app.status = Some(StatusMessage::error(format!("Watcher failed: {e}")));
                    None
                }
            }
        } else {
            debug!("File watcher disabled");
            None
        }
    };

    // Enter terminal
    tui.enter()?;

    // Get theme from config
    let theme = Theme::from_scheme(config.tui.color_scheme);

    // Main event loop
    info!("Entering main event loop");
    let result = run_event_loop(&mut tui, &mut app, &mut watcher, &theme).await;

    // Exit terminal (restore state)
    tui.exit()?;

    // Shutdown watcher gracefully
    if let Some(w) = watcher {
        info!("Shutting down file watcher");
        if let Err(e) = w.shutdown().await {
            error!(error = %e, "Error shutting down watcher");
        }
    }

    result
}

/// Runs the main event loop.
async fn run_event_loop(
    tui: &mut Tui,
    app: &mut App,
    watcher: &mut Option<FileWatcher>,
    theme: &Theme,
) -> Result<(), TuiError> {
    loop {
        // Draw the UI
        tui.draw(|frame| ui::render(app, frame, theme))?;

        // Wait for next event
        let event = tokio::select! {
            // Terminal events
            event = tui.next_event() => event,

            // File watcher events
            file_event = async {
                match watcher {
                    Some(w) => w.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                file_event.map(Event::FileChanged)
            }
        };

        // Process event
        if let Some(event) = event {
            let action = match event {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => app.handle_mouse(mouse),
                Event::Resize { width, height } => {
                    app.set_terminal_size(ratatui::layout::Rect::new(0, 0, width, height));
                    Action::Render
                }
                Event::FileChanged(file_event) => app.handle_file_change(file_event),
                Event::Tick => {
                    app.tick();
                    Action::None
                }
                Event::Render => Action::Render,
                Event::FocusGained | Event::FocusLost => Action::None,
            };

            // Apply action
            app.update(action);

            if let Some(root) = app.take_watcher_restart() {
                if let Some(existing) = watcher.take() {
                    if let Err(e) = existing.shutdown().await {
                        error!(error = %e, "Error shutting down watcher");
                    }
                }

                info!(path = %root, "Restarting file watcher");
                match FileWatcher::new(&root, &app.config.watch, TypeScriptFilter::default()).await {
                    Ok(w) => *watcher = Some(w),
                    Err(e) => {
                        error!(error = %e, "Failed to restart file watcher");
                        app.status = Some(StatusMessage::error(format!("Watcher failed: {e}")));
                        *watcher = None;
                    }
                }
            }
        }

        // Check for quit
        if app.should_quit {
            info!("Quit requested");
            break;
        }
    }

    Ok(())
}

/// Runs the TUI application without file watching.
///
/// This is a simplified version of [`run`] that doesn't start the file watcher,
/// useful for testing or when watching is not desired.
///
/// # Arguments
///
/// * `config` - The application configuration
/// * `scanner` - The file scanner (pre-configured)
///
/// # Errors
///
/// Returns an error if terminal initialization or scanning fails.
pub async fn run_without_watcher(config: Config, scanner: Scanner) -> Result<(), TuiError> {
    let mut config = config;
    config.watch.enabled = false;
    run(config, scanner).await
}
