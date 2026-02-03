//! Filter input component.
//!
//! Displays a text input overlay for filtering the file list.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::theme::Theme;

/// A filter input overlay widget.
///
/// Displays a centered text input for entering filter text.
/// This is typically shown as a modal overlay when filter mode is active.
pub struct FilterInput<'a> {
    /// The current filter text.
    text: &'a str,
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> FilterInput<'a> {
    /// Creates a new filter input widget.
    #[must_use]
    pub const fn new(text: &'a str, theme: &'a Theme) -> Self {
        Self { text, theme }
    }
}

impl Widget for &FilterInput<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the area first for overlay effect
        Clear.render(area, buf);

        // Build the input content with cursor
        let input_content = if self.text.is_empty() {
            Line::from(vec![
                Span::styled(
                    "Type to filter...",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled("▌", Style::default().fg(self.theme.accent)),
            ])
        } else {
            Line::from(vec![
                Span::styled(self.text, self.theme.base_style()),
                Span::styled("▌", Style::default().fg(self.theme.accent)),
            ])
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.focused_border_style)
            .title(Span::styled(
                " Filter (Esc to cancel, Enter to confirm) ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Rgb(30, 30, 40)));

        let paragraph = Paragraph::new(input_content)
            .block(block)
            .alignment(ratatui::layout::Alignment::Left);

        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_input_new() {
        let theme = Theme::dark();
        let input = FilterInput::new("test", &theme);
        assert_eq!(input.text, "test");
    }

    #[test]
    fn test_filter_input_empty() {
        let theme = Theme::dark();
        let input = FilterInput::new("", &theme);
        assert!(input.text.is_empty());
    }
}
