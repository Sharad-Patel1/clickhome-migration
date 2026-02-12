//! Graph artifact loading and summary helpers.
//!
//! Reads `graph.json` and `migration-plan.json` artifacts produced by the
//! `ch-migrate graph` command and normalizes them for TUI rendering.

use std::fmt;

use camino::{Utf8Path, Utf8PathBuf};
use ch_core::{CstAnchor, ModelReference};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use thiserror::Error;

/// Default artifact directory name.
pub const DEFAULT_GRAPH_ARTIFACT_DIR: &str = "graph-artifacts";

/// Errors encountered while loading graph artifacts.
#[derive(Debug, Error)]
pub enum GraphArtifactError {
    /// Required artifact file is missing.
    #[error("graph artifact missing: {path}")]
    Missing {
        /// Missing artifact path.
        path: Utf8PathBuf,
    },

    /// Failed to read an artifact file.
    #[error("failed to read graph artifact {path}: {source}")]
    Read {
        /// Artifact path.
        path: Utf8PathBuf,
        /// Read error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse artifact JSON.
    #[error("failed to parse graph artifact {path}: {source}")]
    Parse {
        /// Artifact path.
        path: Utf8PathBuf,
        /// Parse error.
        #[source]
        source: serde_json::Error,
    },
}

/// Normalized artifacts for TUI rendering.
#[derive(Debug, Clone)]
pub struct GraphArtifacts {
    /// Summary metrics for the graph and plan.
    pub summary: GraphSummary,
    /// Normalized graph nodes.
    pub nodes: Vec<GraphNodeSummary>,
    /// Normalized graph edges.
    pub edges: Vec<GraphEdgeSummary>,
    /// Normalized migration plan steps.
    pub steps: Vec<PlanStepSummary>,
    /// Snapshot mode label from `graph.json`.
    pub graph_snapshot_mode: String,
    /// Snapshot mode label from `migration-plan.json`.
    pub plan_snapshot_mode: String,
    /// Whether plan steps include node ids.
    pub plan_has_node_ids: bool,
}

impl GraphArtifacts {
    /// Loads graph artifacts from the given directory.
    pub fn load(dir: &Utf8Path) -> Result<Self, GraphArtifactError> {
        let graph_path = dir.join("graph.json");
        let plan_path = dir.join("migration-plan.json");

        let graph_json: GraphArtifactJson = read_json(&graph_path)?;
        let plan_json: PlanArtifactJson = read_json(&plan_path)?;
        Ok(Self::from_json(graph_json, plan_json))
    }

    /// Returns the display label for a node id.
    #[must_use]
    pub fn node_label(&self, node_id: &str) -> String {
        self.nodes
            .iter()
            .find(|node| node.node_id == node_id)
            .map_or_else(|| node_id.to_owned(), |node| node.display_name.clone())
    }

    /// Returns an iterator of plan steps associated with a node id.
    pub fn steps_for_node(&self, node_id: &str) -> impl Iterator<Item = &PlanStepSummary> {
        self.steps
            .iter()
            .filter(move |step| step.node_ids.iter().any(|id| id == node_id))
    }

    fn from_json(graph: GraphArtifactJson, plan: PlanArtifactJson) -> Self {
        let nodes = GraphNodeSummary::from_graph(&graph.graph);
        let edges = GraphEdgeSummary::from_graph(&graph.graph);
        let steps = PlanStepSummary::from_plan(&plan);

        let plan_has_node_ids = steps.iter().any(|step| !step.node_ids.is_empty());

        let summary = GraphSummary::new(&graph, &plan, &steps);

        Self {
            summary,
            nodes,
            edges,
            steps,
            graph_snapshot_mode: graph.snapshot_mode,
            plan_snapshot_mode: plan.snapshot_mode,
            plan_has_node_ids,
        }
    }
}

/// Summary metrics extracted from graph and plan artifacts.
#[derive(Debug, Clone)]
pub struct GraphSummary {
    /// Total graph nodes.
    pub total_nodes: usize,
    /// Total graph edges.
    pub total_edges: usize,
    /// Total residual legacy usages.
    pub residuals: usize,
    /// Matched mappings count.
    pub matched: usize,
    /// Low-confidence mappings count.
    pub low_confidence: usize,
    /// No-match mappings count.
    pub no_match: usize,
    /// Plan step count.
    pub plan_steps: usize,
    /// Top risk steps (highest `risk_score_bps`).
    pub top_risk_steps: Vec<PlanRiskStep>,
}

impl GraphSummary {
    fn new(graph: &GraphArtifactJson, plan: &PlanArtifactJson, steps: &[PlanStepSummary]) -> Self {
        let mut top_risk_steps: Vec<_> = steps
            .iter()
            .map(|step| PlanRiskStep {
                step_id: step.step_id.clone(),
                order: step.order,
                risk_score_bps: step.risk_score_bps,
            })
            .collect();
        top_risk_steps.sort_by(|left, right| {
            right
                .risk_score_bps
                .cmp(&left.risk_score_bps)
                .then_with(|| left.order.cmp(&right.order))
                .then_with(|| left.step_id.cmp(&right.step_id))
        });
        top_risk_steps.truncate(3);

        Self {
            total_nodes: graph.metadata.counts.total_nodes,
            total_edges: graph.metadata.counts.total_edges,
            residuals: graph.diff.counts.residuals,
            matched: graph.diff.counts.matched,
            low_confidence: graph.diff.counts.low_confidence,
            no_match: graph.diff.counts.no_match,
            plan_steps: plan.steps.len(),
            top_risk_steps,
        }
    }
}

/// Compact view of a plan step risk score.
#[derive(Debug, Clone)]
pub struct PlanRiskStep {
    /// Step identifier.
    pub step_id: String,
    /// Step order.
    pub order: u32,
    /// Risk score in basis points.
    pub risk_score_bps: u32,
}

impl fmt::Display for PlanRiskStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} bps)", self.step_id, self.risk_score_bps)
    }
}

/// Summary of a graph node for UI display.
#[derive(Debug, Clone)]
pub struct GraphNodeSummary {
    /// Stable node identifier.
    pub node_id: String,
    /// Display name derived from node metadata.
    pub display_name: String,
    /// Node kind label.
    pub kind: Option<String>,
    /// Source label (`shared_legacy`, `shared_2023`, etc.).
    pub source: Option<String>,
    /// Canonical id when available.
    pub canonical_id: Option<String>,
    /// Model name when available.
    pub model_name: Option<String>,
    /// Symbol name when available.
    pub symbol_name: Option<String>,
    /// Category label when available.
    pub category: Option<String>,
    /// Export name when available.
    pub export_name: Option<String>,
    /// Definition path when available.
    pub definition_path: Option<Utf8PathBuf>,
    /// File path when available.
    pub file_path: Option<Utf8PathBuf>,
    /// Source classification label.
    pub source_classification: Option<String>,
}

impl GraphNodeSummary {
    fn from_graph(graph: &GraphArtifactGraph) -> Vec<Self> {
        let mut nodes = Vec::new();
        if let Some(ref entries) = graph.nodes {
            nodes = entries
                .iter()
                .map(|entry| {
                    let display_name = entry
                        .model_name
                        .clone()
                        .or_else(|| entry.symbol_name.clone())
                        .or_else(|| entry.canonical_id.clone())
                        .unwrap_or_else(|| entry.node_id.clone());
                    Self {
                        node_id: entry.node_id.clone(),
                        display_name,
                        kind: entry.kind.clone(),
                        source: entry.source.clone(),
                        canonical_id: entry.canonical_id.clone(),
                        model_name: entry.model_name.clone(),
                        symbol_name: entry.symbol_name.clone(),
                        category: entry.category.clone(),
                        export_name: entry.export_name.clone(),
                        definition_path: entry.definition_path.clone(),
                        file_path: entry.file_path.clone(),
                        source_classification: entry.source_classification.clone(),
                    }
                })
                .collect();
        } else if let Some(ref node_ids) = graph.node_ids {
            nodes = node_ids
                .iter()
                .map(|node_id| Self {
                    node_id: node_id.clone(),
                    display_name: node_id.clone(),
                    kind: None,
                    source: None,
                    canonical_id: None,
                    model_name: None,
                    symbol_name: None,
                    category: None,
                    export_name: None,
                    definition_path: None,
                    file_path: None,
                    source_classification: None,
                })
                .collect();
        }

        nodes.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        nodes
    }
}

/// Summary of a graph edge for drilldown.
#[derive(Debug, Clone)]
pub struct GraphEdgeSummary {
    /// Source node id.
    pub source_node_id: String,
    /// Target node id.
    pub target_node_id: String,
    /// Edge kind label.
    pub kind: String,
    /// Evidence count.
    pub evidence_count: usize,
    /// Evidence entries (full snapshot only).
    pub evidence: Vec<GraphEvidenceSummary>,
    /// Anchor hashes (minimal snapshot only).
    pub anchor_hashes: Vec<u64>,
}

impl GraphEdgeSummary {
    fn from_graph(graph: &GraphArtifactGraph) -> Vec<Self> {
        graph
            .edges
            .iter()
            .map(|edge| Self {
                source_node_id: edge.source_node_id.clone(),
                target_node_id: edge.target_node_id.clone(),
                kind: edge.kind.clone(),
                evidence_count: edge.evidence_count,
                evidence: edge.evidence.clone().unwrap_or_default(),
                anchor_hashes: edge.anchor_hashes.clone().unwrap_or_default(),
            })
            .collect()
    }
}

/// Evidence entry for a graph edge.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphEvidenceSummary {
    /// Relation label.
    pub relation: String,
    /// Source model reference.
    pub source: ModelReference,
    /// Target model reference.
    pub target: ModelReference,
    /// Evidence anchors.
    pub anchors: Vec<CstAnchor>,
}

/// Summary of a migration plan step.
#[derive(Debug, Clone)]
pub struct PlanStepSummary {
    /// Step identifier.
    pub step_id: String,
    /// Step order.
    pub order: u32,
    /// Component identifier.
    pub component_id: String,
    /// Prerequisite step ids.
    pub prerequisites: Vec<String>,
    /// Risk score in basis points.
    pub risk_score_bps: u32,
    /// Node ids included in this step (full snapshot only).
    pub node_ids: Vec<String>,
    /// Impacted file count.
    pub impacted_file_count: usize,
    /// Impacted file paths (full snapshot only).
    pub impacted_files: Vec<Utf8PathBuf>,
}

impl PlanStepSummary {
    fn from_plan(plan: &PlanArtifactJson) -> Vec<Self> {
        plan.steps
            .iter()
            .map(|step| {
                let impacted_files = step.impacted_files.clone().unwrap_or_default();
                let impacted_file_count = step.impacted_file_count.unwrap_or(impacted_files.len());
                Self {
                    step_id: step.step_id.clone(),
                    order: step.order,
                    component_id: step.component_id.clone(),
                    prerequisites: step.prerequisites.clone(),
                    risk_score_bps: step.risk_score_bps,
                    node_ids: step.node_ids.clone().unwrap_or_default(),
                    impacted_file_count,
                    impacted_files,
                }
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct GraphArtifactJson {
    snapshot_mode: String,
    metadata: GraphArtifactMetadata,
    graph: GraphArtifactGraph,
    diff: GraphArtifactDiff,
}

#[derive(Debug, Deserialize)]
struct GraphArtifactMetadata {
    counts: GraphArtifactCounts,
}

#[derive(Debug, Deserialize)]
struct GraphArtifactCounts {
    total_nodes: usize,
    total_edges: usize,
}

#[derive(Debug, Deserialize)]
struct GraphArtifactGraph {
    #[serde(default)]
    nodes: Option<Vec<GraphArtifactNode>>,
    #[serde(default)]
    node_ids: Option<Vec<String>>,
    edges: Vec<GraphArtifactEdge>,
}

#[derive(Debug, Deserialize)]
struct GraphArtifactNode {
    node_id: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    source_classification: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    canonical_id: Option<String>,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    symbol_name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    export_name: Option<String>,
    #[serde(default)]
    definition_path: Option<Utf8PathBuf>,
    #[serde(default)]
    file_path: Option<Utf8PathBuf>,
}

#[derive(Debug, Deserialize)]
struct GraphArtifactEdge {
    source_node_id: String,
    target_node_id: String,
    kind: String,
    evidence_count: usize,
    #[serde(default)]
    evidence: Option<Vec<GraphEvidenceSummary>>,
    #[serde(default)]
    anchor_hashes: Option<Vec<u64>>,
}

#[derive(Debug, Deserialize)]
struct GraphArtifactDiff {
    counts: GraphArtifactDiffCounts,
}

#[derive(Debug, Deserialize)]
struct GraphArtifactDiffCounts {
    matched: usize,
    low_confidence: usize,
    no_match: usize,
    residuals: usize,
}

#[derive(Debug, Deserialize)]
struct PlanArtifactJson {
    snapshot_mode: String,
    steps: Vec<PlanArtifactStep>,
}

#[derive(Debug, Deserialize)]
struct PlanArtifactStep {
    step_id: String,
    order: u32,
    component_id: String,
    prerequisites: Vec<String>,
    risk_score_bps: u32,
    #[serde(default)]
    node_ids: Option<Vec<String>>,
    #[serde(default)]
    impacted_file_count: Option<usize>,
    #[serde(default)]
    impacted_files: Option<Vec<Utf8PathBuf>>,
}

fn read_json<T: DeserializeOwned>(path: &Utf8Path) -> Result<T, GraphArtifactError> {
    if !path.exists() {
        return Err(GraphArtifactError::Missing {
            path: path.to_path_buf(),
        });
    }

    let raw =
        std::fs::read_to_string(path.as_std_path()).map_err(|source| GraphArtifactError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    serde_json::from_str(&raw).map_err(|source| GraphArtifactError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAPH_MINIMAL: &str = r#"{
  "snapshot_mode": "minimal",
  "metadata": {
    "counts": {
      "total_nodes": 3,
      "total_edges": 2
    }
  },
  "graph": {
    "node_ids": ["node-a", "node-b"],
    "edges": [
      {
        "source_node_id": "node-a",
        "target_node_id": "node-b",
        "kind": "imports_symbol",
        "evidence_count": 1,
        "anchor_hashes": [101, 202]
      }
    ]
  },
  "diff": {
    "counts": {
      "matched": 4,
      "low_confidence": 1,
      "no_match": 0,
      "residuals": 2
    }
  }
}"#;

    const PLAN_MINIMAL: &str = r#"{
  "snapshot_mode": "minimal",
  "steps": [
    {
      "step_id": "step-1",
      "order": 1,
      "component_id": "component-a",
      "prerequisites": [],
      "risk_score_bps": 250,
      "impacted_file_count": 3
    },
    {
      "step_id": "step-2",
      "order": 2,
      "component_id": "component-b",
      "prerequisites": ["step-1"],
      "risk_score_bps": 400,
      "impacted_file_count": 1
    }
  ]
}"#;

    #[test]
    fn test_from_json_builds_summary() {
        let Ok(graph) = serde_json::from_str(GRAPH_MINIMAL) else {
            assert!(false, "failed to parse graph json fixture");
            return;
        };
        let Ok(plan) = serde_json::from_str(PLAN_MINIMAL) else {
            assert!(false, "failed to parse plan json fixture");
            return;
        };
        let artifacts = GraphArtifacts::from_json(graph, plan);

        assert_eq!(artifacts.summary.total_nodes, 3);
        assert_eq!(artifacts.summary.total_edges, 2);
        assert_eq!(artifacts.summary.residuals, 2);
        assert_eq!(artifacts.summary.plan_steps, 2);
        assert_eq!(artifacts.summary.top_risk_steps.len(), 2);
        assert_eq!(artifacts.summary.top_risk_steps[0].step_id, "step-2");
    }

    #[test]
    fn test_node_labels_fallback_to_id() {
        let Ok(graph) = serde_json::from_str(GRAPH_MINIMAL) else {
            assert!(false, "failed to parse graph json fixture");
            return;
        };
        let Ok(plan) = serde_json::from_str(PLAN_MINIMAL) else {
            assert!(false, "failed to parse plan json fixture");
            return;
        };
        let artifacts = GraphArtifacts::from_json(graph, plan);

        assert_eq!(artifacts.node_label("node-a"), "node-a");
        assert_eq!(artifacts.node_label("missing"), "missing");
    }
}
