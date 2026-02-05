//! Statistics panel component.
//!
//! Displays migration statistics and progress gauge.
//! During active scans, shows a scanning progress indicator.

use ch_scanner::StatsSnapshot;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Widget};

use crate::app::ScanState;
use crate::theme::Theme;

/// The statistics panel component.
///
/// Displays:
/// - During scanning: Progress bar with "Scanning X/Y files"
/// - After scan: Legacy, Partial, Migrated, No Models counts with migration gauge
pub struct StatsPanel<'a> {
    /// Statistics snapshot.
    stats: &'a StatsSnapshot,
    /// Current scan state for progress display.
    scan_state: &'a ScanState,
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> StatsPanel<'a> {
    /// Creates a new stats panel.
    #[must_use]
    pub const fn new(
        stats: &'a StatsSnapshot,
        scan_state: &'a ScanState,
        theme: &'a Theme,
    ) -> Self {
        Self {
            stats,
            scan_state,
            theme,
        }
    }
}

impl Widget for &StatsPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray));

        // Split into stats text and gauge
        let inner = block.inner(area);
        block.render(area, buf);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(30)])
            .split(inner);

        // Show scanning progress OR migration stats based on scan state
        if let ScanState::Scanning {
            discovered,
            scanned,
        } = self.scan_state
        {
            // Render scanning progress
            render_scanning_progress(*discovered, *scanned, &chunks, buf);
        } else {
            // Render normal migration stats
            render_migration_stats(self.stats, &chunks, buf, self.theme);
        }
    }
}

/// Renders the scanning progress view.
fn render_scanning_progress(discovered: usize, scanned: usize, chunks: &[Rect], buf: &mut Buffer) {
    // Scanning status text
    let scanning_line = Line::from(vec![
        Span::styled(
            "Scanning... ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{scanned}/{discovered} files"),
            Style::default().fg(Color::White),
        ),
    ]);

    let status_paragraph = Paragraph::new(scanning_line);
    status_paragraph.render(chunks[0], buf);

    // Scanning progress gauge
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let progress_percent = if discovered > 0 {
        ((scanned as f64 / discovered as f64) * 100.0).round() as u16
    } else {
        0
    };

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
        .percent(progress_percent)
        .label(format!("{progress_percent}%"));

    gauge.render(chunks[1], buf);
}

/// Renders the normal migration statistics view.
fn render_migration_stats(stats: &StatsSnapshot, chunks: &[Rect], buf: &mut Buffer, theme: &Theme) {
    // Render stats counts
    let stats_line = Line::from(vec![
        Span::styled("Legacy: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", stats.legacy),
            Style::default().fg(theme.legacy_fg),
        ),
        Span::raw(" │ "),
        Span::styled("Partial: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", stats.partial),
            Style::default().fg(theme.partial_fg),
        ),
        Span::raw(" │ "),
        Span::styled("Migrated: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", stats.migrated),
            Style::default().fg(theme.migrated_fg),
        ),
        Span::raw(" │ "),
        Span::styled("No Models: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", stats.no_models),
            Style::default().fg(theme.no_models_fg),
        ),
    ]);

    let stats_paragraph = Paragraph::new(stats_line);
    stats_paragraph.render(chunks[0], buf);

    // Render progress gauge
    // progress_percent() returns 0.0-100.0, which fits safely in u16
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let progress_u16 = stats.progress_percent().round() as u16;

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(theme.migrated_fg).bg(Color::DarkGray))
        .percent(progress_u16)
        .label(format!("{:.1}%", stats.progress_percent()));

    gauge.render(chunks[1], buf);
}
