//! Event types for the TUI event loop.
//!
//! This module provides the [`Event`] enum representing all events
//! that can be processed by the TUI application.
//!
//! # Event Sources
//!
//! Events originate from multiple sources:
//!
//! - **Terminal**: Key presses, mouse events, window resizing
//! - **File Watcher**: File change notifications from `ch-watcher`
//! - **Scanner**: Streaming scan results from `ch-scanner`
//! - **Timer**: Periodic tick events for animations and updates
//!
//! # Example
//!
//! ```ignore
//! use ch_tui::Event;
//!
//! loop {
//!     match tui.next_event().await {
//!         Some(Event::Key(key)) => handle_key(key),
//!         Some(Event::FileChanged(event)) => handle_file_change(event),
//!         Some(Event::ScanUpdate(update)) => handle_scan_update(update),
//!         Some(Event::Tick) => update_animations(),
//!         None => break,
//!     }
//! }
//! ```

use ch_scanner::ScanUpdate;
use ch_watcher::FileEvent;
use crossterm::event::{KeyEvent, MouseEvent};

/// Events that can be processed by the TUI.
///
/// This enum unifies all event sources into a single type that can be
/// processed by the application's main event loop.
#[derive(Debug)]
#[non_exhaustive]
pub enum Event {
    /// A key press event from the terminal.
    Key(KeyEvent),

    /// A mouse event from the terminal.
    Mouse(MouseEvent),

    /// Terminal window was resized.
    Resize {
        /// New width in columns.
        width: u16,
        /// New height in rows.
        height: u16,
    },

    /// A file changed in the watched directory.
    FileChanged(FileEvent),

    /// Scan progress update from background task.
    ///
    /// These events are streamed from the background scanner and include
    /// file discovery counts, individual file results, and completion status.
    ScanUpdate(ScanUpdate),

    /// Periodic tick for animations and updates.
    ///
    /// The tick rate is configured via [`TuiConfig::tick_rate_ms`].
    Tick,

    /// Signal to render a new frame.
    ///
    /// This is separate from Tick to allow different rates for
    /// UI updates vs animations.
    Render,

    /// Focus gained by the terminal window.
    FocusGained,

    /// Focus lost by the terminal window.
    FocusLost,
}

impl Event {
    /// Returns `true` if this is a key event.
    #[inline]
    #[must_use]
    pub const fn is_key(&self) -> bool {
        matches!(self, Self::Key(_))
    }

    /// Returns `true` if this is a file change event.
    #[inline]
    #[must_use]
    pub const fn is_file_changed(&self) -> bool {
        matches!(self, Self::FileChanged(_))
    }

    /// Returns `true` if this is a scan update event.
    #[inline]
    #[must_use]
    pub const fn is_scan_update(&self) -> bool {
        matches!(self, Self::ScanUpdate(_))
    }

    /// Returns `true` if this is a tick event.
    #[inline]
    #[must_use]
    pub const fn is_tick(&self) -> bool {
        matches!(self, Self::Tick)
    }

    /// Returns `true` if this is a render event.
    #[inline]
    #[must_use]
    pub const fn is_render(&self) -> bool {
        matches!(self, Self::Render)
    }

    /// Returns the key event if this is a Key variant.
    #[inline]
    #[must_use]
    pub const fn as_key(&self) -> Option<&KeyEvent> {
        match self {
            Self::Key(key) => Some(key),
            _ => None,
        }
    }

    /// Returns the file event if this is a `FileChanged` variant.
    #[inline]
    #[must_use]
    pub const fn as_file_changed(&self) -> Option<&FileEvent> {
        match self {
            Self::FileChanged(event) => Some(event),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn test_event_is_key() {
        let key_event = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(key_event.is_key());

        let tick_event = Event::Tick;
        assert!(!tick_event.is_key());
    }

    #[test]
    fn test_event_is_file_changed() {
        let file_event = Event::FileChanged(FileEvent::new(Utf8PathBuf::from("test.ts")));
        assert!(file_event.is_file_changed());

        let tick_event = Event::Tick;
        assert!(!tick_event.is_file_changed());
    }

    #[test]
    fn test_event_is_tick() {
        let tick = Event::Tick;
        assert!(tick.is_tick());

        let render = Event::Render;
        assert!(!render.is_tick());
    }

    #[test]
    fn test_event_as_key() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let event = Event::Key(key);
        assert!(event.as_key().is_some());
        assert_eq!(event.as_key().map(|k| k.code), Some(KeyCode::Enter));

        let tick = Event::Tick;
        assert!(tick.as_key().is_none());
    }

    #[test]
    fn test_event_as_file_changed() {
        let file_event = FileEvent::new(Utf8PathBuf::from("test.ts"));
        let event = Event::FileChanged(file_event);
        assert!(event.as_file_changed().is_some());

        let tick = Event::Tick;
        assert!(tick.as_file_changed().is_none());
    }

    #[test]
    fn test_resize_event() {
        let event = Event::Resize {
            width: 120,
            height: 40,
        };
        if let Event::Resize { width, height } = event {
            assert_eq!(width, 120);
            assert_eq!(height, 40);
        } else {
            panic!("Expected Resize event");
        }
    }
}
