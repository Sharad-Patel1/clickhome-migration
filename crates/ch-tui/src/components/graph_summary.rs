//! Graph summary panel component.
//!
//! Displays summary metrics from graph artifacts, along with top-risk steps.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::graph_artifacts::GraphSummary;
use crate::theme::Theme;

/// Graph summary panel component.
pub struct GraphSummaryPanel<'a> {
    summary: Option<&'a GraphSummary>,
    error: Option<&'a str>,
    theme: &'a Theme,
}

impl<'a> GraphSummaryPanel<'a> {
    /// Creates a new graph summary panel.
    #[must_use]
    pub const fn new(
        summary: Option<&'a GraphSummary>,
        error: Option<&'a str>,
        theme: &'a Theme,
    ) -> Self {
        Self { summary, error, theme }
    }

    fn build_lines(&self) -> Vec<Line<'a>> {
        if let Some(summary) = self.summary {
            let top_risk = if summary.top_risk_steps.is_empty() {
                "none".to_owned()
            } else {
                summary
                    .top_risk_steps
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            vec![
                Line::from(vec![
                    Span::styled("Graph: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!(
                            "{} nodes │ {} edges │ {} residuals",
                            summary.total_nodes, summary.total_edges, summary.residuals
                        ),
                        self.theme.base_style(),
                    ),
                    Span::raw(" │ "),
                    Span::styled(
                        format!(
                            "Matched: {} / Low: {} / None: {}",
                            summary.matched, summary.low_confidence, summary.no_match
                        ),
                        self.theme.dimmed_style(),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Plan: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{} steps", summary.plan_steps), self.theme.base_style()),
                    Span::raw(" │ "),
                    Span::styled("Top risk: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(top_risk, self.theme.accent_style()),
                ]),
            ]
        } else if let Some(error) = self.error {
            vec![
                Line::from(vec![
                    Span::styled("Graph artifacts unavailable", self.theme.dimmed_style()),
                    Span::raw(" │ "),
                    Span::styled(error, Style::default().fg(Color::DarkGray)),
                ]),
                Line::from(Span::styled(
                    "Run `ch-migrate graph` to generate ./graph-artifacts",
                    self.theme.dimmed_style(),
                )),
            ]
        } else {
            vec![
                Line::from(Span::styled("Graph artifacts not loaded", self.theme.dimmed_style())),
                Line::from(Span::styled(
                    "Run `ch-migrate graph` to generate ./graph-artifacts",
                    self.theme.dimmed_style(),
                )),
            ]
        }
    }
}

impl Widget for &GraphSummaryPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Graph Summary ",
                Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        block.render(area, buf);

        let lines = self.build_lines();
        let content = Text::from(lines);
        Paragraph::new(content).render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_lines_without_summary() {
        let theme = Theme::dark();
        let panel = GraphSummaryPanel::new(None, None, &theme);
        let lines = panel.build_lines();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_build_lines_with_summary() {
        let theme = Theme::dark();
        let summary = GraphSummary {
            total_nodes: 3,
            total_edges: 2,
            residuals: 1,
            matched: 4,
            low_confidence: 1,
            no_match: 0,
            plan_steps: 2,
            top_risk_steps: vec![],
        };
        let panel = GraphSummaryPanel::new(Some(&summary), None, &theme);
        let lines = panel.build_lines();
        assert_eq!(lines.len(), 2);
    }
}
