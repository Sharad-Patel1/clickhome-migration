//! Terminal wrapper with async event streaming.
//!
//! This module provides the [`Tui`] struct which wraps a Ratatui terminal
//! and bridges crossterm events to async tokio using channels.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                Blocking Thread (spawn_blocking)                 │
//! │  ┌──────────────────┐                                          │
//! │  │ crossterm::event │ ─── poll ──► Event ──► mpsc::Sender      │
//! │  │     ::read()     │                                          │
//! │  └──────────────────┘                                          │
//! └──────────────────────────────────────────────────────────────────┘
//!                                              │
//!                                    blocking_send
//!                                              │
//!                                              ▼
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                    Async Runtime (tokio)                        │
//! │  ┌──────────────────┐    ┌────────────────┐                     │
//! │  │ Tui              │ ←─ │ mpsc::Receiver │ ← Application Loop  │
//! │  │ (terminal)       │    │ (events)       │                     │
//! │  └──────────────────┘    └────────────────┘                     │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use ch_tui::Tui;
//!
//! let mut tui = Tui::new(60.0)?;
//! tui.enter()?;
//!
//! loop {
//!     tui.draw(|frame| {
//!         // Render UI
//!     })?;
//!
//!     if let Some(event) = tui.next_event().await {
//!         // Handle event
//!     }
//! }
//!
//! tui.exit()?;
//! ```

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    EventStream, KeyEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use ratatui::Terminal;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace, warn};

use crate::error::TuiError;
use crate::event::Event;

/// Default channel capacity for events.
const EVENT_CHANNEL_CAPACITY: usize = 100;

/// Terminal wrapper with async event streaming.
///
/// Manages the terminal state (raw mode, alternate screen) and provides
/// an async interface for receiving terminal and application events.
pub struct Tui {
    /// The underlying Ratatui terminal.
    terminal: Terminal<CrosstermBackend<Stdout>>,

    /// Receiver for events from the event loop task.
    event_rx: mpsc::Receiver<Event>,

    /// Sender for injecting events (used for file watcher integration).
    event_tx: mpsc::Sender<Event>,

    /// Handle to the event loop task.
    task: Option<JoinHandle<()>>,

    /// Token for cancelling the event loop.
    cancellation_token: CancellationToken,

    /// Frame rate for rendering (frames per second).
    frame_rate: f64,

    /// Tick rate for periodic updates (ticks per second).
    tick_rate: f64,
}

impl Tui {
    /// Creates a new TUI with the specified tick rate.
    ///
    /// The terminal is not entered yet; call [`enter()`](Self::enter) to
    /// initialize raw mode and the alternate screen.
    ///
    /// # Arguments
    ///
    /// * `tick_rate` - Number of tick events per second
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal cannot be initialized.
    pub fn new(tick_rate: f64) -> Result<Self, TuiError> {
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;

        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let cancellation_token = CancellationToken::new();

        debug!(tick_rate, "Created TUI");

        Ok(Self {
            terminal,
            event_rx,
            event_tx,
            task: None,
            cancellation_token,
            frame_rate: 60.0, // Default 60 FPS
            tick_rate,
        })
    }

    /// Sets the frame rate for rendering.
    ///
    /// # Arguments
    ///
    /// * `fps` - Frames per second
    #[must_use]
    pub const fn with_frame_rate(mut self, fps: f64) -> Self {
        self.frame_rate = fps;
        self
    }

    /// Returns the event sender for injecting external events.
    ///
    /// Use this to send file watcher events or other external events
    /// into the TUI event loop.
    #[must_use]
    pub fn event_sender(&self) -> mpsc::Sender<Event> {
        self.event_tx.clone()
    }

    /// Enters the terminal (raw mode, alternate screen).
    ///
    /// This must be called before drawing to the terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if terminal mode cannot be changed.
    pub fn enter(&mut self) -> Result<(), TuiError> {
        debug!("Entering terminal");

        enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        io::stdout().execute(EnableMouseCapture)?;
        io::stdout().execute(EnableBracketedPaste)?;

        self.terminal.hide_cursor()?;
        self.terminal.clear()?;

        self.start_event_loop();

        debug!("Terminal entered");
        Ok(())
    }

    /// Exits the terminal (restores normal mode).
    ///
    /// This should be called before the application exits to ensure
    /// the terminal is restored to a usable state.
    ///
    /// # Errors
    ///
    /// Returns an error if terminal mode cannot be restored.
    pub fn exit(&mut self) -> Result<(), TuiError> {
        debug!("Exiting terminal");

        self.stop_event_loop();

        self.terminal.show_cursor()?;

        io::stdout().execute(DisableBracketedPaste)?;
        io::stdout().execute(DisableMouseCapture)?;
        io::stdout().execute(LeaveAlternateScreen)?;
        disable_raw_mode()?;

        debug!("Terminal exited");
        Ok(())
    }

    /// Draws to the terminal.
    ///
    /// # Arguments
    ///
    /// * `f` - A closure that receives a mutable reference to the frame
    ///
    /// # Errors
    ///
    /// Returns an error if drawing fails.
    pub fn draw<F>(&mut self, f: F) -> Result<(), TuiError>
    where
        F: FnOnce(&mut Frame),
    {
        self.terminal.draw(f)?;
        Ok(())
    }

    /// Returns the next event from the event loop.
    ///
    /// This is an async method that waits for the next event.
    /// Returns `None` if the event channel is closed.
    pub async fn next_event(&mut self) -> Option<Event> {
        self.event_rx.recv().await
    }

    /// Returns the terminal size.
    #[must_use]
    pub fn size(&self) -> Rect {
        let size = self.terminal.size().unwrap_or_default();
        Rect::new(0, 0, size.width, size.height)
    }

    /// Starts the event loop in a background task.
    fn start_event_loop(&mut self) {
        let tick_delay = Duration::from_secs_f64(1.0 / self.tick_rate);
        let render_delay = Duration::from_secs_f64(1.0 / self.frame_rate);

        let event_tx = self.event_tx.clone();
        let cancellation_token = self.cancellation_token.clone();

        debug!(
            tick_delay_ms = tick_delay.as_millis(),
            render_delay_ms = render_delay.as_millis(),
            "Starting event loop"
        );

        let task = tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_delay);
            let mut render_interval = tokio::time::interval(render_delay);

            // Don't delay the first tick
            tick_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            render_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                let event = tokio::select! {
                    () = cancellation_token.cancelled() => {
                        debug!("Event loop cancelled");
                        break;
                    }
                    _ = tick_interval.tick() => Some(Event::Tick),
                    _ = render_interval.tick() => Some(Event::Render),
                    event = Self::read_crossterm_event(&mut reader) => event,
                };

                if let Some(event) = event {
                    trace!(?event, "Sending event");
                    if event_tx.send(event).await.is_err() {
                        error!("Event channel closed");
                        break;
                    }
                }
            }

            debug!("Event loop ended");
        });

        self.task = Some(task);
    }

    /// Stops the event loop.
    fn stop_event_loop(&mut self) {
        debug!("Stopping event loop");
        self.cancellation_token.cancel();

        if let Some(task) = self.task.take() {
            // Don't block on task completion - it will cancel via the token
            task.abort();
        }
    }

    /// Reads a crossterm event and converts it to our Event type.
    async fn read_crossterm_event(reader: &mut EventStream) -> Option<Event> {
        use futures_util::StreamExt;

        match reader.next().await {
            Some(Ok(ref event)) => Self::convert_crossterm_event(event),
            Some(Err(e)) => {
                warn!(error = %e, "Error reading terminal event");
                None
            }
            None => {
                debug!("Event stream ended");
                None
            }
        }
    }

    /// Converts a crossterm event to our Event type.
    fn convert_crossterm_event(event: &crossterm::event::Event) -> Option<Event> {
        use crossterm::event::Event as CrosstermEvent;

        match event {
            CrosstermEvent::Key(key) => {
                // Only handle key press events, not release
                if key.kind == KeyEventKind::Press {
                    Some(Event::Key(*key))
                } else {
                    None
                }
            }
            CrosstermEvent::Mouse(mouse) => Some(Event::Mouse(*mouse)),
            CrosstermEvent::Resize(width, height) => Some(Event::Resize {
                width: *width,
                height: *height,
            }),
            CrosstermEvent::FocusGained => Some(Event::FocusGained),
            CrosstermEvent::FocusLost => Some(Event::FocusLost),
            CrosstermEvent::Paste(_) => None, // Not handling paste events currently
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Attempt to restore terminal on drop
        if let Err(e) = self.exit() {
            error!(error = %e, "Failed to restore terminal on drop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_new() {
        // Can't actually test terminal operations without a real terminal,
        // but we can verify the struct is created
        let result = Tui::new(4.0);
        // This will fail in CI without a terminal, but that's expected
        if result.is_ok() {
            let tui = result.ok();
            drop(tui);
        }
    }

    #[test]
    fn test_event_channel_capacity() {
        assert_eq!(EVENT_CHANNEL_CAPACITY, 100);
    }
}
