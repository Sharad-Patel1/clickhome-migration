//! Help panel component.
//!
//! Displays a modal overlay with key bindings and help information.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Clear, Row, Table, Widget};

use crate::theme::Theme;

/// Key binding definition for the help panel.
struct KeyBinding {
    /// The key(s) to press.
    key: &'static str,
    /// Description of what the key does.
    description: &'static str,
    /// The mode(s) where this binding applies.
    mode: &'static str,
}

/// Static list of key bindings to display.
const KEY_BINDINGS: &[KeyBinding] = &[
    // Navigation
    KeyBinding {
        key: "j / ↓",
        description: "Next file",
        mode: "Normal",
    },
    KeyBinding {
        key: "k / ↑",
        description: "Previous file",
        mode: "Normal",
    },
    KeyBinding {
        key: "g / Home",
        description: "Go to first file",
        mode: "Normal",
    },
    KeyBinding {
        key: "G / End",
        description: "Go to last file",
        mode: "Normal",
    },
    KeyBinding {
        key: "PgDn / PgUp",
        description: "Page down / up",
        mode: "Normal",
    },
    KeyBinding {
        key: "Tab",
        description: "Toggle focus (List/Details)",
        mode: "Normal",
    },
    // Filtering
    KeyBinding {
        key: "/",
        description: "Start filter mode",
        mode: "Normal",
    },
    KeyBinding {
        key: "f",
        description: "Cycle status filter",
        mode: "Normal",
    },
    KeyBinding {
        key: "Esc",
        description: "Clear filter / Exit mode",
        mode: "Filter/Help",
    },
    KeyBinding {
        key: "Enter",
        description: "Confirm filter",
        mode: "Filter",
    },
    // Actions
    KeyBinding {
        key: "r",
        description: "Rescan all files",
        mode: "Normal",
    },
    KeyBinding {
        key: "o",
        description: "Open file in editor",
        mode: "Normal",
    },
    KeyBinding {
        key: "d",
        description: "Configure directories",
        mode: "Normal",
    },
    KeyBinding {
        key: "?",
        description: "Toggle help panel",
        mode: "Normal",
    },
    KeyBinding {
        key: "q / Ctrl+c",
        description: "Quit",
        mode: "Any",
    },
];

/// A help panel overlay widget.
///
/// Displays key bindings in a table format as a modal overlay.
pub struct HelpPanel<'a> {
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> HelpPanel<'a> {
    /// Creates a new help panel.
    #[must_use]
    pub const fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    /// Builds the table rows from key bindings.
    fn build_rows(&self) -> Vec<Row<'static>> {
        KEY_BINDINGS
            .iter()
            .map(|binding| {
                Row::new(vec![
                    Cell::from(Span::styled(
                        binding.key,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Cell::from(Span::styled(
                        binding.description,
                        self.theme.base_style(),
                    )),
                    Cell::from(Span::styled(
                        binding.mode,
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
            })
            .collect()
    }
}

impl Widget for &HelpPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the area first for overlay effect
        Clear.render(area, buf);

        // Block with title
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.focused_border_style)
            .title(Span::styled(
                " Help - Key Bindings ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Rgb(25, 25, 35)));

        // Column headers
        let header = Row::new(vec![
            Cell::from(Span::styled(
                "Key",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )),
            Cell::from(Span::styled(
                "Action",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )),
            Cell::from(Span::styled(
                "Mode",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )),
        ])
        .height(1)
        .bottom_margin(1);

        // Table rows
        let rows = self.build_rows();

        // Column widths
        let widths = [
            Constraint::Length(15),
            Constraint::Min(25),
            Constraint::Length(12),
        ];

        // Build table
        let table = Table::new(rows, widths)
            .block(block)
            .header(header)
            .row_highlight_style(Style::default());

        table.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_panel_new() {
        let theme = Theme::dark();
        let _panel = HelpPanel::new(&theme);
    }

    #[test]
    fn test_key_bindings_not_empty() {
        assert!(!KEY_BINDINGS.is_empty());
    }
}
