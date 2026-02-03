//! Header bar component.
//!
//! Displays the application title, project path, and file count.
//! During active scans, shows a scanning indicator.

use ch_core::Config;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::app::ScanState;

/// The header bar component.
///
/// Displays:
/// - Application title
/// - Project path
/// - Total file count (or scanning indicator)
/// - Help indicator
pub struct HeaderBar<'a> {
    /// The configuration (for project path).
    config: &'a Config,
    /// Total number of files scanned.
    file_count: usize,
    /// Current scan state for scanning indicator.
    scan_state: &'a ScanState,
}

impl<'a> HeaderBar<'a> {
    /// Creates a new header bar.
    #[must_use]
    pub const fn new(config: &'a Config, file_count: usize, scan_state: &'a ScanState) -> Self {
        Self {
            config,
            file_count,
            scan_state,
        }
    }
}

impl Widget for &HeaderBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let path_style = Style::default().fg(Color::White);
        let count_style = Style::default().fg(Color::Green);
        let scanning_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_style = Style::default().fg(Color::Yellow);

        let project_path = self.config.scan.root_path.as_str();
        let path_display = if project_path.is_empty() {
            "<no project>".to_owned()
        } else if project_path.len() > 40 {
            format!("...{}", &project_path[project_path.len() - 37..])
        } else {
            project_path.to_owned()
        };

        // Build status indicator based on scan state
        let status_span = match self.scan_state {
            ScanState::Scanning { scanned, .. } => {
                Span::styled(format!("Scanning... ({scanned} files)"), scanning_style)
            }
            ScanState::Idle | ScanState::Complete => {
                Span::styled(format!("{} files", self.file_count), count_style)
            }
        };

        let line = Line::from(vec![
            Span::styled("ch-migrate", title_style),
            Span::raw(" │ "),
            Span::styled(path_display, path_style),
            Span::raw(" │ "),
            status_span,
            Span::raw(" │ "),
            Span::styled("? for help", help_style),
        ]);

        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray));

        let paragraph = Paragraph::new(line).block(block);
        paragraph.render(area, buf);
    }
}
