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
use ratatui::widgets::{Clear, Widget};
use ratatui::Frame;

use crate::app::{App, AppMode};
use crate::components::{
    DetailPane, DirectoryInput, FileListView, FilterInput, HeaderBar, HelpPanel, StatsPanel,
    StatusBar,
};
use crate::theme::Theme;

/// Renders the entire UI based on the current application state.
pub fn render(app: &mut App, frame: &mut Frame, theme: &Theme) {
    let area = frame.area();
    Clear.render(area, frame.buffer_mut());

    // Main vertical layout:
    // - Header (2 lines)
    // - Stats Panel (2 lines)
    // - Main Content (flexible)
    // - Status Bar (1 line)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Header
            Constraint::Length(2), // Stats
            Constraint::Min(6),    // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    // Render header
    let header = HeaderBar::new(&app.config, app.file_count(), &app.scan_state);
    frame.render_widget(&header, main_chunks[0]);

    // Render stats panel
    let stats_panel = StatsPanel::new(&app.stats, &app.scan_state, theme);
    frame.render_widget(&stats_panel, main_chunks[1]);

    // Render main content (file list + details)
    render_main_content(app, frame, main_chunks[2], theme);

    // Render status bar
    let status_bar = StatusBar::new(app, theme);
    frame.render_widget(&status_bar, main_chunks[3]);

    // Render filter input overlay if in filter mode
    if app.mode == AppMode::Filtering {
        let filter_input = FilterInput::new(&app.filter.text, theme);
        let filter_area = centered_rect_fixed_height(50, 5, area);
        frame.render_widget(&filter_input, filter_area);
    }

    // Render help panel overlay if in help mode
    if app.mode == AppMode::Help {
        let help_panel = HelpPanel::new(theme);
        let help_area = centered_rect_percent(60, 70, area);
        frame.render_widget(&help_panel, help_area);
    }

    // Render directory setup overlay if active
    if app.mode == AppMode::DirectorySetup {
        let dir_input = DirectoryInput::new(&app.directory_setup, theme);
        let dir_area = centered_rect_percent(80, 30, area);
        frame.render_widget(&dir_input, dir_area);
    }
}

/// Renders the main content area (file list and detail pane).
fn render_main_content(app: &mut App, frame: &mut Frame, area: Rect, theme: &Theme) {
    // Split horizontally: file list (60%) | details (40%)
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Render file list with persistent state.
    {
        let (files, filter_active, focused, state) = app.file_list_render_data();
        let file_list = FileListView::new(files, filter_active, focused, theme);
        frame.render_stateful_widget(&file_list, content_chunks[0], state);
    }

    // Render detail pane with persistent scroll state.
    {
        let (selected_file, focused, state) = app.detail_render_data();
        let detail_pane = DetailPane::new(selected_file, focused, theme);
        frame.render_stateful_widget(&detail_pane, content_chunks[1], state);
    }
}

/// Creates a centered rectangle with percentage-based width and height.
fn centered_rect_percent(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }

    let clamped_x = percent_x.clamp(1, 100);
    let clamped_y = percent_y.clamp(1, 100);

    let width = area.width.saturating_mul(clamped_x) / 100;
    let height = area.height.saturating_mul(clamped_y) / 100;

    centered_rect_size(width.max(1), height.max(1), area)
}

/// Creates a centered rectangle with percentage width and fixed row height.
fn centered_rect_fixed_height(percent_x: u16, rows: u16, area: Rect) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }

    let clamped_x = percent_x.clamp(1, 100);
    let width = area.width.saturating_mul(clamped_x) / 100;
    let height = rows.clamp(1, area.height);

    centered_rect_size(width.max(1), height, area)
}

/// Creates a centered rectangle with explicit width and height, clamped to the area.
fn centered_rect_size(width: u16, height: u16, area: Rect) -> Rect {
    let clamped_width = width.clamp(1, area.width);
    let clamped_height = height.clamp(1, area.height);
    let x = area.x + (area.width.saturating_sub(clamped_width) / 2);
    let y = area.y + (area.height.saturating_sub(clamped_height) / 2);

    Rect::new(x, y, clamped_width, clamped_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_centered_rect_percent() {
        let area = Rect::new(0, 0, 100, 100);
        let centered = centered_rect_percent(50, 50, area);

        // Should be roughly centered
        assert!(centered.x > 0);
        assert!(centered.y > 0);
        assert!(centered.width < area.width);
        assert!(centered.height < area.height);
    }

    #[test]
    fn test_centered_rect_fixed_height_for_filter_popup() {
        let area = Rect::new(0, 0, 120, 40);
        let popup = centered_rect_fixed_height(50, 5, area);

        assert_eq!(popup.height, 5);
        assert_eq!(popup.width, 60);
        assert!(popup.x > 0);
        assert!(popup.y > 0);
    }

    #[test]
    fn test_centered_rect_fixed_height_clamps_on_tiny_terminal() {
        let area = Rect::new(0, 0, 8, 3);
        let popup = centered_rect_fixed_height(50, 5, area);

        assert!(popup.width > 0);
        assert!(popup.height > 0);
        assert!(popup.width <= area.width);
        assert!(popup.height <= area.height);
    }
}
