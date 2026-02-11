//! Detail pane component.
//!
//! Displays detailed information about the selected file, including
//! its imports and model references.

use ch_core::FileInfo;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
    Widget, Wrap,
};

use crate::app::DetailPaneState;
use crate::theme::Theme;

/// A stateful detail pane widget.
///
/// Displays detailed information about the selected file:
/// - File path and name
/// - Migration status
/// - Legacy imports list
/// - Migrated imports list
/// - Model references
///
/// Uses [`StatefulWidget`] to maintain scroll state.
pub struct DetailPane<'a> {
    /// The selected file (if any).
    file: Option<&'a FileInfo>,
    /// Whether this widget has focus.
    focused: bool,
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> DetailPane<'a> {
    /// Creates a new detail pane.
    #[must_use]
    pub const fn new(file: Option<&'a FileInfo>, focused: bool, theme: &'a Theme) -> Self {
        Self {
            file,
            focused,
            theme,
        }
    }

    /// Renders the "no selection" placeholder.
    fn render_placeholder(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.border_style)
            .title(Span::styled(" Details ", self.theme.header_style));

        let text = Text::from(vec![
            Line::from(""),
            Line::from(Span::styled("No file selected", self.theme.dimmed_style())),
            Line::from(""),
            Line::from(Span::styled(
                "Select a file from the list",
                self.theme.dimmed_style(),
            )),
            Line::from(Span::styled(
                "to view its details.",
                self.theme.dimmed_style(),
            )),
        ]);

        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);

        paragraph.render(area, buf);
    }

    /// Renders the file details.
    fn render_details(
        &self,
        file: &FileInfo,
        area: Rect,
        buf: &mut Buffer,
        state: &mut DetailPaneState,
    ) {
        let border_style = if self.focused {
            self.theme.focused_border_style
        } else {
            self.theme.border_style
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(" Details ", self.theme.header_style));

        let inner = block.inner(area);
        block.render(area, buf);

        // Build content lines
        let mut lines = Vec::new();

        // File name
        let file_name = file.path.file_name().unwrap_or(file.path.as_str());
        lines.push(Line::from(vec![
            Span::styled("File: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                file_name.to_owned(),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        // Full path
        lines.push(Line::from(vec![
            Span::styled("Path: ", Style::default().fg(Color::DarkGray)),
            Span::styled(file.path.as_str(), self.theme.base_style()),
        ]));

        // Status
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(file.status.label(), self.theme.status_style(file.status)),
        ]));

        // Separator
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── Imports ───",
            Style::default().fg(Color::DarkGray),
        )));

        // Legacy imports
        let legacy_imports: Vec<_> = file.legacy_imports().collect();
        if legacy_imports.is_empty() {
            lines.push(Line::from(Span::styled(
                "No legacy imports",
                self.theme.dimmed_style(),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Legacy: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} imports", legacy_imports.len()),
                    Style::default().fg(self.theme.legacy_fg),
                ),
            ]));
            for import in &legacy_imports {
                for name in &import.names {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled("•", Style::default().fg(self.theme.legacy_fg)),
                        Span::raw(" "),
                        Span::styled(name.clone(), self.theme.base_style()),
                    ]));
                }
            }
        }

        // Migrated imports
        let migrated_imports: Vec<_> = file.migrated_imports().collect();
        if migrated_imports.is_empty() {
            lines.push(Line::from(Span::styled(
                "No migrated imports",
                self.theme.dimmed_style(),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Migrated: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} imports", migrated_imports.len()),
                    Style::default().fg(self.theme.migrated_fg),
                ),
            ]));
            for import in &migrated_imports {
                for name in &import.names {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled("•", Style::default().fg(self.theme.migrated_fg)),
                        Span::raw(" "),
                        Span::styled(name.clone(), self.theme.base_style()),
                    ]));
                }
            }
        }

        // Model references section
        if !file.model_refs.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "─── Model References ───",
                Style::default().fg(Color::DarkGray),
            )));

            for model_ref in &file.model_refs {
                // Determine style based on source
                let source_style = if model_ref.is_legacy() {
                    Style::default().fg(self.theme.legacy_fg)
                } else {
                    Style::default().fg(self.theme.migrated_fg)
                };

                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("•", Style::default().fg(self.theme.accent)),
                    Span::raw(" "),
                    Span::styled(model_ref.name.clone(), self.theme.base_style()),
                    Span::raw(" "),
                    Span::styled(format!("[{}]", model_ref.source.dir_name()), source_style),
                ]));
            }
        }

        // Create paragraph with scrolling
        let content = Text::from(lines.clone());
        let total_lines = lines.len();

        // Clamp scroll offset
        let max_scroll = total_lines.saturating_sub(inner.height as usize);
        if state.scroll_offset > max_scroll {
            state.scroll_offset = max_scroll;
        }

        // Terminal scroll offset is bounded by terminal height, which is always < 65535
        #[allow(clippy::cast_possible_truncation)]
        let scroll_offset = state.scroll_offset as u16;

        let paragraph = Paragraph::new(content)
            .scroll((scroll_offset, 0))
            .wrap(Wrap { trim: false });

        paragraph.render(inner, buf);

        // Render scrollbar if content overflows
        if total_lines > inner.height as usize {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            let mut scrollbar_state = ScrollbarState::new(total_lines)
                .position(state.scroll_offset)
                .viewport_content_length(inner.height as usize);

            scrollbar.render(
                inner.inner(ratatui::layout::Margin {
                    vertical: 1,
                    horizontal: 0,
                }),
                buf,
                &mut scrollbar_state,
            );
        }
    }
}

impl StatefulWidget for &DetailPane<'_> {
    type State = DetailPaneState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        match self.file {
            Some(file) => self.render_details(file, area, buf, state),
            None => self.render_placeholder(area, buf),
        }
    }
}
