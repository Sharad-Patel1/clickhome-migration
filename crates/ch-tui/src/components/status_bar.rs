//! Status bar component.
//!
//! Displays status messages, mode indicators, and help hints.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::{App, AppMode};
use crate::theme::Theme;

/// The status bar component.
///
/// Displays:
/// - Current mode indicator
/// - Status message (if any)
/// - Filter indicator (if active)
/// - Help hint
pub struct StatusBar<'a> {
    /// The application state.
    app: &'a App,
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> StatusBar<'a> {
    /// Creates a new status bar.
    #[must_use]
    pub const fn new(app: &'a App, theme: &'a Theme) -> Self {
        Self { app, theme }
    }

    /// Builds the status line spans.
    fn build_line(&self) -> Line<'a> {
        let mut spans = Vec::new();

        // Mode indicator
        let mode_text = match self.app.mode {
            AppMode::Normal => "NORMAL",
            AppMode::Filtering => "FILTER",
            AppMode::Help => "HELP",
        };
        spans.push(Span::styled(
            format!(" {mode_text} "),
            Style::default()
                .fg(Color::Black)
                .bg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));

        // Status message
        if let Some(ref status) = self.app.status {
            let style = if status.is_error {
                Style::default().fg(self.theme.error_fg)
            } else {
                Style::default().fg(self.theme.fg)
            };
            spans.push(Span::styled(status.text.clone(), style));
            spans.push(Span::raw(" │ "));
        }

        // Filter indicator
        if self.app.filter.is_active() {
            spans.push(Span::styled("Filter: ", Style::default().fg(Color::DarkGray)));
            if !self.app.filter.text.is_empty() {
                spans.push(Span::styled(
                    format!("\"{}\"", self.app.filter.text),
                    Style::default().fg(Color::Yellow),
                ));
                spans.push(Span::raw(" "));
            }
            if let Some(status) = self.app.filter.status {
                spans.push(Span::styled(
                    status.label(),
                    self.theme.status_style(status),
                ));
            }
            spans.push(Span::raw(" │ "));
        }

        // File count
        spans.push(Span::styled(
            format!("{}/{}", self.app.filtered_count(), self.app.file_count()),
            Style::default().fg(Color::DarkGray),
        ));

        Line::from(spans)
    }
}

impl Widget for &StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let line = self.build_line();
        let paragraph = Paragraph::new(line).style(self.theme.status_bar_style);
        paragraph.render(area, buf);
    }
}
