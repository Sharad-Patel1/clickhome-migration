//! Directory setup input component.
//!
//! Displays a modal overlay for configuring root/shared paths.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::app::{DirectoryField, DirectorySetup};
use crate::theme::Theme;

/// Directory setup overlay widget.
pub struct DirectoryInput<'a> {
    setup: &'a DirectorySetup,
    theme: &'a Theme,
}

impl<'a> DirectoryInput<'a> {
    /// Creates a new directory input widget.
    #[must_use]
    pub const fn new(setup: &'a DirectorySetup, theme: &'a Theme) -> Self {
        Self { setup, theme }
    }
}

impl Widget for &DirectoryInput<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.focused_border_style)
            .title(Span::styled(
                " Directories (Tab to switch, Enter to apply, Esc to cancel) ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Rgb(30, 30, 40)));

        let root = build_field_line(
            "WebApp.Desktop/src",
            &self.setup.root_input,
            self.setup.active_field == DirectoryField::Root,
            self.theme,
        );
        let shared = build_field_line(
            "shared",
            &self.setup.shared_input,
            self.setup.active_field == DirectoryField::Shared,
            self.theme,
        );
        let shared_2023 = build_field_line(
            "shared_2023",
            &self.setup.shared_2023_input,
            self.setup.active_field == DirectoryField::Shared2023,
            self.theme,
        );

        let lines = vec![root, shared, shared_2023];
        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }
}

fn build_field_line<'a>(
    label: &'a str,
    value: &'a str,
    focused: bool,
    theme: &'a Theme,
) -> Line<'a> {
    let label_style = if focused {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let value_style = if focused {
        theme.base_style()
    } else {
        Style::default().fg(Color::Gray)
    };

    let display_value = if value.is_empty() { "<unset>" } else { value };

    let mut spans = vec![
        Span::styled(format!("{label}: "), label_style),
        Span::styled(display_value, value_style),
    ];

    if focused {
        spans.push(Span::styled("▌", Style::default().fg(theme.accent)));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::DirectoryField;

    #[test]
    fn test_directory_input_new() {
        let theme = Theme::dark();
        let setup = DirectorySetup {
            root_input: "/tmp/root".to_owned(),
            shared_input: "/tmp/shared".to_owned(),
            shared_2023_input: "/tmp/shared_2023".to_owned(),
            active_field: DirectoryField::Root,
        };

        let _input = DirectoryInput::new(&setup, &theme);
    }
}
