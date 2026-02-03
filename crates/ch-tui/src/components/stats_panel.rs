//! Statistics panel component.
//!
//! Displays migration statistics and progress gauge.

use ch_scanner::StatsSnapshot;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Widget};

use crate::theme::Theme;

/// The statistics panel component.
///
/// Displays:
/// - Legacy file count
/// - Partial migration count
/// - Migrated file count
/// - Progress gauge
pub struct StatsPanel<'a> {
    /// Statistics snapshot.
    stats: &'a StatsSnapshot,
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> StatsPanel<'a> {
    /// Creates a new stats panel.
    #[must_use]
    pub const fn new(stats: &'a StatsSnapshot, theme: &'a Theme) -> Self {
        Self { stats, theme }
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

        // Render stats counts
        let stats_line = Line::from(vec![
            Span::styled("Legacy: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", self.stats.legacy),
                Style::default().fg(self.theme.legacy_fg),
            ),
            Span::raw(" │ "),
            Span::styled("Partial: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", self.stats.partial),
                Style::default().fg(self.theme.partial_fg),
            ),
            Span::raw(" │ "),
            Span::styled("Migrated: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", self.stats.migrated),
                Style::default().fg(self.theme.migrated_fg),
            ),
            Span::raw(" │ "),
            Span::styled("No Models: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", self.stats.no_models),
                Style::default().fg(self.theme.no_models_fg),
            ),
        ]);

        let stats_paragraph = Paragraph::new(stats_line);
        stats_paragraph.render(chunks[0], buf);

        // Render progress gauge
        // progress_percent() returns 0.0-100.0, which fits safely in u16
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let progress_u16 = self.stats.progress_percent().round() as u16;

        let gauge = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(self.theme.migrated_fg)
                    .bg(Color::DarkGray),
            )
            .percent(progress_u16)
            .label(format!("{:.1}%", self.stats.progress_percent()));

        gauge.render(chunks[1], buf);
    }
}
