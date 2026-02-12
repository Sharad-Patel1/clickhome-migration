//! Model list and drilldown components for graph view.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, HighlightSpacing, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, StatefulWidget, Table, TableState, Widget, Wrap,
};

use crate::app::{FileListState, GraphDrilldownState};
use crate::graph_artifacts::{GraphArtifacts, GraphEdgeSummary, GraphNodeSummary};
use crate::theme::Theme;

/// Model list view for graph browsing.
pub struct ModelListView<'a> {
    nodes: &'a [GraphNodeSummary],
    focused: bool,
    theme: &'a Theme,
}

impl<'a> ModelListView<'a> {
    /// Creates a new model list view.
    #[must_use]
    pub const fn new(nodes: &'a [GraphNodeSummary], focused: bool, theme: &'a Theme) -> Self {
        Self { nodes, focused, theme }
    }

    fn build_rows(&self, state: &FileListState) -> Vec<Row<'a>> {
        let indices = state.filtered_indices();
        let node_indices: Vec<usize> =
            indices.map_or_else(|| (0..self.nodes.len()).collect(), <[usize]>::to_vec);

        node_indices.into_iter().map(|idx| self.build_row(&self.nodes[idx])).collect()
    }

    fn build_row(&self, node: &GraphNodeSummary) -> Row<'a> {
        let name = truncate_text(&node.display_name, 48);
        let source = format_source_label(node.source.as_deref());
        let kind = node.kind.as_deref().unwrap_or("-");

        let cells = vec![
            Cell::from(Span::styled(name, self.theme.base_style())),
            Cell::from(Span::styled(source, self.theme.dimmed_style())),
            Cell::from(Span::styled(kind.to_owned(), self.theme.dimmed_style())),
        ];

        Row::new(cells).height(1)
    }
}

impl StatefulWidget for &ModelListView<'_> {
    type State = FileListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let inner_height = area.height.saturating_sub(2);
        state.visible_height = inner_height as usize;

        let border_style =
            if self.focused { self.theme.focused_border_style } else { self.theme.border_style };

        let title = format!(" Models ({}) ", self.nodes.len());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(title, self.theme.header_style));

        let rows = self.build_rows(state);
        let widths = [Constraint::Min(24), Constraint::Length(14), Constraint::Length(12)];

        let table = Table::new(rows, widths)
            .block(block)
            .row_highlight_style(self.theme.highlight_style)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_symbol("▸ ");

        let mut table_state = TableState::default();
        table_state.select(state.selected);
        *table_state.offset_mut() = state.scroll_offset;

        StatefulWidget::render(table, area, buf, &mut table_state);

        state.selected = table_state.selected();
        state.scroll_offset = table_state.offset();
    }
}

/// Drilldown panel for a selected model.
pub struct ModelDrilldown<'a> {
    artifacts: Option<&'a GraphArtifacts>,
    selected: Option<&'a GraphNodeSummary>,
    focused: bool,
    theme: &'a Theme,
}

impl<'a> ModelDrilldown<'a> {
    /// Creates a new model drilldown panel.
    #[must_use]
    pub const fn new(
        artifacts: Option<&'a GraphArtifacts>,
        selected: Option<&'a GraphNodeSummary>,
        focused: bool,
        theme: &'a Theme,
    ) -> Self {
        Self { artifacts, selected, focused, theme }
    }

    fn build_lines(&self) -> Vec<Line<'a>> {
        let Some(artifacts) = self.artifacts else {
            return vec![
                Line::from(""),
                Line::from(Span::styled("Graph artifacts not loaded", self.theme.dimmed_style())),
                Line::from(Span::styled(
                    "Run `ch-migrate graph` to generate ./graph-artifacts",
                    self.theme.dimmed_style(),
                )),
            ];
        };

        let Some(node) = self.selected else {
            return vec![
                Line::from(""),
                Line::from(Span::styled("No model selected", self.theme.dimmed_style())),
                Line::from(Span::styled(
                    "Select a model to view dependencies",
                    self.theme.dimmed_style(),
                )),
            ];
        };

        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Model: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                node.display_name.clone(),
                Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD),
            ),
        ]));

        let kind = node.kind.as_deref().unwrap_or("unknown");
        let source = format_source_label(node.source.as_deref());
        lines.push(Line::from(vec![
            Span::styled("Kind: ", Style::default().fg(Color::DarkGray)),
            Span::styled(kind.to_owned(), self.theme.base_style()),
            Span::raw(" │ "),
            Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
            Span::styled(source, self.theme.base_style()),
        ]));

        if let Some(ref canonical) = node.canonical_id {
            lines.push(Line::from(vec![
                Span::styled("Canonical: ", Style::default().fg(Color::DarkGray)),
                Span::styled(canonical.clone(), self.theme.base_style()),
            ]));
        }

        if let Some(ref path) = node.definition_path {
            lines.push(Line::from(vec![
                Span::styled("Definition: ", Style::default().fg(Color::DarkGray)),
                Span::styled(path.as_str(), self.theme.base_style()),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "── Dependencies ──",
            Style::default().fg(Color::DarkGray),
        )));

        let (inbound, outbound) = split_edges(artifacts, &node.node_id);
        lines.push(Line::from(vec![
            Span::styled("Inbound: ", Style::default().fg(Color::DarkGray)),
            Span::styled(inbound.len().to_string(), self.theme.base_style()),
            Span::raw(" │ "),
            Span::styled("Outbound: ", Style::default().fg(Color::DarkGray)),
            Span::styled(outbound.len().to_string(), self.theme.base_style()),
        ]));

        append_edge_preview("Inbound", &inbound, artifacts, &mut lines, self.theme);
        append_edge_preview("Outbound", &outbound, artifacts, &mut lines, self.theme);

        append_evidence_section(&inbound, &outbound, &mut lines, self.theme);
        append_plan_context(artifacts, &node.node_id, &mut lines, self.theme);

        lines
    }
}

impl StatefulWidget for &ModelDrilldown<'_> {
    type State = GraphDrilldownState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let border_style =
            if self.focused { self.theme.focused_border_style } else { self.theme.border_style };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(" Drilldown ", self.theme.header_style));

        let inner = block.inner(area);
        block.render(area, buf);

        let lines = self.build_lines();
        let total_lines = lines.len();
        let max_scroll = total_lines.saturating_sub(inner.height as usize);
        if state.scroll_offset > max_scroll {
            state.scroll_offset = max_scroll;
        }

        #[allow(clippy::cast_possible_truncation)]
        let scroll_offset = state.scroll_offset as u16;

        let content = Text::from(lines);
        let paragraph =
            Paragraph::new(content).scroll((scroll_offset, 0)).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);

        if total_lines > inner.height as usize {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            let mut scrollbar_state = ScrollbarState::new(total_lines)
                .position(state.scroll_offset)
                .viewport_content_length(inner.height as usize);

            scrollbar.render(
                inner.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
                buf,
                &mut scrollbar_state,
            );
        }
    }
}

fn append_edge_preview<'a>(
    label: &str,
    edges: &[&'a GraphEdgeSummary],
    artifacts: &'a GraphArtifacts,
    lines: &mut Vec<Line<'a>>,
    theme: &'a Theme,
) {
    if edges.is_empty() {
        lines.push(Line::from(Span::styled(format!("{label}: none"), theme.dimmed_style())));
        return;
    }

    lines.push(Line::from(Span::styled(
        format!("{label} (showing up to 5):"),
        Style::default().fg(Color::DarkGray),
    )));

    for edge in edges.iter().take(5) {
        let other = if label == "Inbound" {
            artifacts.node_label(&edge.source_node_id)
        } else {
            artifacts.node_label(&edge.target_node_id)
        };
        lines.push(Line::from(vec![
            Span::raw("  • "),
            Span::styled(other, theme.base_style()),
            Span::raw(" "),
            Span::styled(format!("[{}]", edge.kind), theme.dimmed_style()),
        ]));
    }
}

fn append_evidence_section<'a>(
    inbound: &[&'a GraphEdgeSummary],
    outbound: &[&'a GraphEdgeSummary],
    lines: &mut Vec<Line<'a>>,
    theme: &'a Theme,
) {
    let mut evidence_edges: Vec<&GraphEdgeSummary> = inbound
        .iter()
        .chain(outbound.iter())
        .copied()
        .filter(|edge| !edge.evidence.is_empty() || !edge.anchor_hashes.is_empty())
        .collect();

    if evidence_edges.is_empty() {
        return;
    }

    evidence_edges.truncate(3);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("── Evidence ──", Style::default().fg(Color::DarkGray))));

    for edge in evidence_edges {
        lines.push(Line::from(vec![
            Span::styled("Edge: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} -> {}", edge.source_node_id, edge.target_node_id),
                theme.base_style(),
            ),
            Span::raw(" "),
            Span::styled(format!("[{}]", edge.kind), theme.dimmed_style()),
        ]));

        if !edge.evidence.is_empty() {
            for evidence in edge.evidence.iter().take(1) {
                for anchor in evidence.anchors.iter().take(3) {
                    lines.push(Line::from(vec![
                        Span::raw("  · "),
                        Span::styled(anchor.file_path.as_str(), theme.base_style()),
                        Span::raw(":"),
                        Span::styled(anchor.start.line.to_string(), theme.base_style()),
                        Span::raw(":"),
                        Span::styled(anchor.start.column.to_string(), theme.base_style()),
                        Span::raw(" "),
                        Span::styled(format!("hash {}", anchor.snippet_hash), theme.dimmed_style()),
                    ]));
                }
            }
        } else if !edge.anchor_hashes.is_empty() {
            let hashes = edge
                .anchor_hashes
                .iter()
                .take(4)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(Line::from(vec![
                Span::raw("  · "),
                Span::styled("anchor hashes: ", theme.dimmed_style()),
                Span::styled(hashes, theme.base_style()),
            ]));
        }
    }
}

fn append_plan_context<'a>(
    artifacts: &'a GraphArtifacts,
    node_id: &str,
    lines: &mut Vec<Line<'a>>,
    theme: &'a Theme,
) {
    lines.push(Line::from(""));
    lines
        .push(Line::from(Span::styled("── Plan Context ──", Style::default().fg(Color::DarkGray))));

    if !artifacts.plan_has_node_ids {
        lines.push(Line::from(Span::styled(
            "Plan detail unavailable in minimal snapshot",
            theme.dimmed_style(),
        )));
        return;
    }

    let steps: Vec<_> = artifacts.steps_for_node(node_id).collect();
    if steps.is_empty() {
        lines.push(Line::from(Span::styled(
            "No plan steps reference this model",
            theme.dimmed_style(),
        )));
        return;
    }

    for step in steps.iter().take(3) {
        lines.push(Line::from(vec![
            Span::styled("Step: ", Style::default().fg(Color::DarkGray)),
            Span::styled(step.step_id.clone(), theme.base_style()),
            Span::raw(" │ "),
            Span::styled(format!("order {}", step.order), Style::default().fg(Color::DarkGray)),
            Span::raw(" │ "),
            Span::styled(
                format!("risk {} bps", step.risk_score_bps),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        if !step.prerequisites.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  · "),
                Span::styled("Prereqs: ", Style::default().fg(Color::DarkGray)),
                Span::styled(step.prerequisites.join(", "), theme.base_style()),
            ]));
        }
    }
}

fn split_edges<'a>(
    artifacts: &'a GraphArtifacts,
    node_id: &str,
) -> (Vec<&'a GraphEdgeSummary>, Vec<&'a GraphEdgeSummary>) {
    let mut inbound = Vec::new();
    let mut outbound = Vec::new();

    for edge in &artifacts.edges {
        if edge.target_node_id == node_id {
            inbound.push(edge);
        }
        if edge.source_node_id == node_id {
            outbound.push(edge);
        }
    }

    inbound.sort_by(|left, right| left.source_node_id.cmp(&right.source_node_id));
    outbound.sort_by(|left, right| left.target_node_id.cmp(&right.target_node_id));
    (inbound, outbound)
}

fn truncate_text(text: &str, max_width: usize) -> String {
    if text.len() <= max_width {
        return text.to_owned();
    }
    let ellipsis = "...";
    let available = max_width.saturating_sub(ellipsis.len());
    if available < 8 {
        return format!("{ellipsis}{}", &text[text.len().saturating_sub(available)..]);
    }
    format!("{ellipsis}{}", &text[text.len() - available..])
}

fn format_source_label(source: Option<&str>) -> String {
    match source.unwrap_or("-") {
        "shared_legacy" => "shared".to_owned(),
        "shared_2023" | "shared2023" => "shared_2023".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_source_label() {
        assert_eq!(format_source_label(Some("shared_legacy")), "shared");
        assert_eq!(format_source_label(Some("shared_2023")), "shared_2023");
        assert_eq!(format_source_label(None), "-");
    }
}
