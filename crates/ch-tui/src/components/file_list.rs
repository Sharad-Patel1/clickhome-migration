//! File list component.
//!
//! Displays a scrollable, selectable list of files with their migration status.

use ch_core::FileInfo;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::Span;
use ratatui::widgets::{
    Block, Borders, Cell, HighlightSpacing, Row, StatefulWidget, Table, TableState,
};

use crate::app::{FileListState, FilterState};
use crate::theme::Theme;

/// A stateful file list widget.
///
/// Displays files in a table with:
/// - Selection indicator
/// - File path (truncated if needed)
/// - Migration status badge
///
/// Uses [`StatefulWidget`] to maintain scroll and selection state.
pub struct FileListView<'a> {
    /// The list of files to display.
    files: &'a [FileInfo],
    /// Current filter state for highlighting.
    filter: &'a FilterState,
    /// Whether this widget has focus.
    focused: bool,
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> FileListView<'a> {
    /// Creates a new file list view.
    #[must_use]
    pub const fn new(
        files: &'a [FileInfo],
        filter: &'a FilterState,
        focused: bool,
        theme: &'a Theme,
    ) -> Self {
        Self {
            files,
            filter,
            focused,
            theme,
        }
    }

    /// Builds rows for the table from the file list.
    fn build_rows(&self, state: &FileListState) -> Vec<Row<'a>> {
        let indices = state.filtered_indices();
        let file_indices: Vec<usize> = indices.map_or_else(
            || (0..self.files.len()).collect(),
            <[usize]>::to_vec,
        );

        file_indices
            .into_iter()
            .map(|idx| {
                let file = &self.files[idx];
                self.build_row(file)
            })
            .collect()
    }

    /// Builds a single table row for a file.
    fn build_row(&self, file: &FileInfo) -> Row<'a> {
        // Status indicator
        let status_indicator = Theme::status_indicator(file.status);
        let status_style = self.theme.status_style(file.status);

        // Truncate long paths
        let path_display = truncate_path(file.path.as_str(), 60);

        // Build cells
        let cells = vec![
            Cell::from(Span::styled(status_indicator, status_style)),
            Cell::from(Span::styled(
                path_display,
                self.theme.base_style(),
            )),
            Cell::from(Span::styled(
                file.status.label(),
                status_style,
            )),
        ];

        Row::new(cells).height(1)
    }
}

impl StatefulWidget for &FileListView<'_> {
    type State = FileListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Update visible height for page navigation
        let inner_height = area.height.saturating_sub(2); // Account for borders
        state.visible_height = inner_height as usize;

        // Border style based on focus
        let border_style = if self.focused {
            self.theme.focused_border_style
        } else {
            self.theme.border_style
        };

        let title = if self.filter.is_active() {
            format!(
                " Files ({} filtered) ",
                state.len(self.files.len())
            )
        } else {
            format!(" Files ({}) ", self.files.len())
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(title, self.theme.header_style));

        // Build rows
        let rows = self.build_rows(state);

        // Column widths
        let widths = [
            Constraint::Length(4),  // Status indicator
            Constraint::Min(30),    // Path
            Constraint::Length(12), // Status label
        ];

        // Build table
        let table = Table::new(rows, widths)
            .block(block)
            .row_highlight_style(self.theme.highlight_style)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_symbol("▸ ");

        // Convert FileListState to TableState for rendering
        let mut table_state = TableState::default();
        table_state.select(state.selected);
        *table_state.offset_mut() = state.scroll_offset;

        // Render the table
        StatefulWidget::render(table, area, buf, &mut table_state);
    }
}

/// Truncates a path to fit within the given width.
fn truncate_path(path: &str, max_width: usize) -> String {
    if path.len() <= max_width {
        return path.to_owned();
    }

    // Try to show the end of the path (most relevant)
    let ellipsis = "...";
    let available = max_width.saturating_sub(ellipsis.len());

    if available < 10 {
        // Path is too short, just truncate
        return format!("{ellipsis}{}", &path[path.len().saturating_sub(available)..]);
    }

    // Show as much of the end as possible
    format!("{ellipsis}{}", &path[path.len() - available..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_path_short() {
        let path = "src/foo.ts";
        assert_eq!(truncate_path(path, 20), "src/foo.ts");
    }

    #[test]
    fn test_truncate_path_long() {
        let path = "src/very/long/path/to/some/deeply/nested/component.ts";
        let truncated = truncate_path(path, 30);
        assert!(truncated.starts_with("..."));
        assert!(truncated.len() <= 30);
    }

    #[test]
    fn test_truncate_path_exact() {
        let path = "exactly_twenty_chars";
        assert_eq!(truncate_path(path, 20), path);
    }
}
