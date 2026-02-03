//! Main UI layout and rendering orchestration.
//!
//! This module provides the main [`render`] function that orchestrates
//! rendering of all UI components based on the current application state.
//!
//! # Layout Structure
//!
//! ```text
//! +------------------------------------------------------------------+
//! | Header: ch-migrate | /path/to/project | Scanned: 1234 files | ?  |
//! +------------------------------------------------------------------+
//! | [Stats] Legacy: 45 | Partial: 12 | Migrated: 890 | [====>   ] 67%|
//! +------------------------------------------------------------------+
//! |  File List                          |  Details                    |
//! |  -----------------------------------|  --------------------------  |
//! |  > src/app/foo.component.ts [L]    |  File: foo.component.ts      |
//! |    src/app/bar.service.ts   [M]    |  Status: Legacy              |
//! |    ...                             |  ...                         |
//! +------------------------------------------------------------------+
//! | Status: Watching... | Last update: 2s ago | Press ? for help      |
//! +------------------------------------------------------------------+
//! ```

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app::{App, AppMode, Focus};
use crate::components::{
    DetailPane, FileListView, FilterInput, HeaderBar, HelpPanel, StatsPanel, StatusBar,
};
use crate::theme::Theme;

/// Renders the entire UI based on the current application state.
pub fn render(app: &App, frame: &mut Frame, theme: &Theme) {
    let area = frame.area();

    // Main vertical layout:
    // - Header (3 lines)
    // - Stats Panel (3 lines)
    // - Main Content (flexible)
    // - Status Bar (1 line)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(3),  // Stats
            Constraint::Min(10),    // Main content
            Constraint::Length(1),  // Status bar
        ])
        .split(area);

    // Render header
    let header = HeaderBar::new(&app.config, app.file_count());
    frame.render_widget(&header, main_chunks[0]);

    // Render stats panel
    let stats_panel = StatsPanel::new(&app.stats, theme);
    frame.render_widget(&stats_panel, main_chunks[1]);

    // Render main content (file list + details)
    render_main_content(app, frame, main_chunks[2], theme);

    // Render status bar
    let status_bar = StatusBar::new(app, theme);
    frame.render_widget(&status_bar, main_chunks[3]);

    // Render filter input overlay if in filter mode
    if app.mode == AppMode::Filtering {
        let filter_input = FilterInput::new(&app.filter.text, theme);
        let filter_area = centered_rect(50, 3, area);
        frame.render_widget(&filter_input, filter_area);
    }

    // Render help panel overlay if in help mode
    if app.mode == AppMode::Help {
        let help_panel = HelpPanel::new(theme);
        let help_area = centered_rect(60, 70, area);
        frame.render_widget(&help_panel, help_area);
    }
}

/// Renders the main content area (file list and detail pane).
fn render_main_content(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    // Split horizontally: file list (60%) | details (40%)
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Render file list
    let file_list = FileListView::new(
        app.files(),
        &app.filter,
        app.focus == Focus::FileList,
        theme,
    );
    frame.render_stateful_widget(
        &file_list,
        content_chunks[0],
        &mut app.file_list_state.clone(),
    );

    // Render detail pane
    let detail_pane = DetailPane::new(
        app.selected_file(),
        app.focus == Focus::DetailPane,
        theme,
    );
    frame.render_stateful_widget(
        &detail_pane,
        content_chunks[1],
        &mut app.detail_state.clone(),
    );
}

/// Creates a centered rectangle with the given percentage width and height.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_centered_rect() {
        let area = Rect::new(0, 0, 100, 100);
        let centered = centered_rect(50, 50, area);

        // Should be roughly centered
        assert!(centered.x > 0);
        assert!(centered.y > 0);
        assert!(centered.width < area.width);
        assert!(centered.height < area.height);
    }
}
