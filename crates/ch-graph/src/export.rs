//! Artifact export pipeline for graph and migration planning outputs.
//!
//! This module owns the deterministic artifact contract for:
//! - `graph.json`
//! - `graph.dot`
//! - `migration-plan.json`
//! - `migration-plan.md`
//!
//! `minimal` mode prioritizes compact evidence hashes and counts, while `full`
//! mode includes expanded diagnostics based on persisted graph/comparator/planner
//! contracts.

use std::time::{SystemTime, UNIX_EPOCH};

use camino::{Utf8Path, Utf8PathBuf};
use ch_core::{CstAnchor, EdgeKind};
use serde_json::json;
use thiserror::Error;

use crate::graph::{DependencyGraph, GraphNodeKind};
use crate::mapping::{GraphDiff, MappingReason, MappingReasonKind, MappingStatus};
use crate::planner::{
    MigrationPlan, MigrationStep, RiskComponentScore, RiskSignalKind, SuggestedReplacement,
};

/// Snapshot detail mode for artifact payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphArtifactSnapshotMode {
    /// Emit compact summary-focused payloads.
    Minimal,
    /// Emit expanded payloads with richer graph and evidence diagnostics.
    Full,
}

/// Artifact format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphArtifactFormat {
    /// Emit JSON artifacts (`graph.json`, `migration-plan.json`).
    Json,
    /// Emit Graphviz artifact (`graph.dot`).
    Dot,
    /// Emit markdown artifact (`migration-plan.md`).
    Md,
    /// Emit all supported artifacts.
    All,
}

/// Export errors emitted by artifact writers.
#[derive(Debug, Error)]
pub enum ExportError {
    /// JSON serialization failed.
    #[error("failed to serialize artifact payload: {0}")]
    Serialize(#[from] serde_json::Error),

    /// File write failed.
    #[error("failed to write artifact to {path}: {source}")]
    Write {
        /// Target artifact path.
        path: Utf8PathBuf,
        /// I/O source error.
        #[source]
        source: std::io::Error,
    },
}

/// Emits deterministic graph/planning artifacts for the selected format.
///
/// Returns the sorted list of file paths written for this export pass.
///
/// # Errors
///
/// Returns [`ExportError`] when payload serialization or file I/O fails.
pub fn export_artifacts(
    output_dir: &Utf8Path,
    snapshot_mode: GraphArtifactSnapshotMode,
    format: GraphArtifactFormat,
    graph: &DependencyGraph,
    diff: &GraphDiff,
    plan: &MigrationPlan,
) -> Result<Vec<Utf8PathBuf>, ExportError> {
    let generation = GenerationMetadata::new(snapshot_mode);
    let mut written = Vec::new();

    if matches!(format, GraphArtifactFormat::Json | GraphArtifactFormat::All) {
        written.extend(write_json_artifacts(
            output_dir, generation, graph, diff, plan,
        )?);
    }
    if matches!(format, GraphArtifactFormat::Dot | GraphArtifactFormat::All) {
        written.extend(write_dot_artifact(output_dir, generation, graph)?);
    }
    if matches!(format, GraphArtifactFormat::Md | GraphArtifactFormat::All) {
        written.extend(write_markdown_artifact(
            output_dir, generation, graph, diff, plan,
        )?);
    }

    written.sort();
    Ok(written)
}

#[derive(Debug, Clone, Copy)]
struct GenerationMetadata {
    generated_unix_epoch_ms: u128,
    snapshot_mode: GraphArtifactSnapshotMode,
}

impl GenerationMetadata {
    fn new(snapshot_mode: GraphArtifactSnapshotMode) -> Self {
        let generated_unix_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        Self {
            generated_unix_epoch_ms,
            snapshot_mode,
        }
    }
}

fn write_json_artifacts(
    output_dir: &Utf8Path,
    generation: GenerationMetadata,
    graph: &DependencyGraph,
    diff: &GraphDiff,
    plan: &MigrationPlan,
) -> Result<Vec<Utf8PathBuf>, ExportError> {
    let graph_path = output_dir.join("graph.json");
    let plan_path = output_dir.join("migration-plan.json");

    let graph_payload = graph_json_payload(generation, graph, diff);
    let plan_payload = migration_plan_json_payload(generation, graph, plan);

    write_json_file(graph_path.as_path(), &graph_payload)?;
    write_json_file(plan_path.as_path(), &plan_payload)?;

    Ok(vec![graph_path, plan_path])
}

fn write_dot_artifact(
    output_dir: &Utf8Path,
    generation: GenerationMetadata,
    graph: &DependencyGraph,
) -> Result<Vec<Utf8PathBuf>, ExportError> {
    let graph_path = output_dir.join("graph.dot");
    let graph_dot = render_graph_dot(generation, graph);
    write_text_file(graph_path.as_path(), &graph_dot)?;
    Ok(vec![graph_path])
}

fn write_markdown_artifact(
    output_dir: &Utf8Path,
    generation: GenerationMetadata,
    graph: &DependencyGraph,
    diff: &GraphDiff,
    plan: &MigrationPlan,
) -> Result<Vec<Utf8PathBuf>, ExportError> {
    let plan_path = output_dir.join("migration-plan.md");
    let markdown = render_plan_markdown(generation, graph, diff, plan);
    write_text_file(plan_path.as_path(), &markdown)?;
    Ok(vec![plan_path])
}

fn write_json_file(path: &Utf8Path, payload: &serde_json::Value) -> Result<(), ExportError> {
    let bytes = serde_json::to_vec_pretty(payload)?;
    std::fs::write(path.as_std_path(), bytes).map_err(|source| ExportError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn write_text_file(path: &Utf8Path, payload: &str) -> Result<(), ExportError> {
    std::fs::write(path.as_std_path(), payload).map_err(|source| ExportError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn graph_json_payload(
    generation: GenerationMetadata,
    graph: &DependencyGraph,
    diff: &GraphDiff,
) -> serde_json::Value {
    let metadata = graph.metadata();
    let counts = &metadata.counts;

    let mut edge_kind_counts: Vec<_> = counts
        .edges_by_kind
        .iter()
        .map(|entry| json!({"kind": edge_kind_label(entry.kind), "count": entry.count}))
        .collect();
    edge_kind_counts.sort_by(|left, right| {
        value_string(left, "kind")
            .cmp(&value_string(right, "kind"))
            .then_with(|| value_usize(left, "count").cmp(&value_usize(right, "count")))
    });

    let graph_section = match generation.snapshot_mode {
        GraphArtifactSnapshotMode::Minimal => {
            let mut node_ids: Vec<_> = graph
                .graph()
                .node_indices()
                .map(|node_index| graph.graph()[node_index].node_id.clone())
                .collect();
            node_ids.sort();
            json!({
                "node_ids": node_ids,
                "edges": collect_graph_edges(graph, generation.snapshot_mode),
            })
        }
        GraphArtifactSnapshotMode::Full => json!({
            "nodes": collect_graph_nodes(graph, true),
            "edges": collect_graph_edges(graph, generation.snapshot_mode),
        }),
    };

    let diff_section = match generation.snapshot_mode {
        GraphArtifactSnapshotMode::Minimal => json!({
            "counts": {
                "matched": diff.counts.matched,
                "low_confidence": diff.counts.low_confidence,
                "no_match": diff.counts.no_match,
                "residuals": diff.counts.residuals,
            },
            "residual_anchor_hashes": collect_residual_anchor_hashes(diff),
        }),
        GraphArtifactSnapshotMode::Full => json!({
            "counts": {
                "matched": diff.counts.matched,
                "low_confidence": diff.counts.low_confidence,
                "no_match": diff.counts.no_match,
                "residuals": diff.counts.residuals,
            },
            "mappings": collect_full_mappings(diff),
            "residual_legacy_usages": collect_full_residuals(diff),
        }),
    };

    json!({
        "snapshot_mode": snapshot_mode_label(generation.snapshot_mode),
        "metadata": {
            "generated_unix_epoch_ms": generation.generated_unix_epoch_ms,
            "counts": {
                "total_nodes": counts.total_nodes,
                "total_edges": counts.total_edges,
                "interface_nodes": counts.interface_nodes,
                "model_nodes": counts.model_nodes,
                "service_nodes": counts.service_nodes,
                "file_nodes": counts.file_nodes,
                "symbol_nodes": counts.symbol_nodes,
                "edges_by_kind": edge_kind_counts,
            },
            "source_roots": metadata.source_roots,
            "parser_metadata": {
                "parser_version": metadata.parser_metadata.parser_version,
                "relation_query_version": metadata.parser_metadata.relation_query_version,
            },
            "object_counts": {
                "mappings": diff.mappings.len(),
                "residual_legacy_usages": diff.residual_legacy_usages.len(),
            },
        },
        "graph": graph_section,
        "diff": diff_section,
    })
}

fn migration_plan_json_payload(
    generation: GenerationMetadata,
    graph: &DependencyGraph,
    plan: &MigrationPlan,
) -> serde_json::Value {
    let steps = match generation.snapshot_mode {
        GraphArtifactSnapshotMode::Minimal => collect_minimal_plan_steps(plan),
        GraphArtifactSnapshotMode::Full => collect_full_plan_steps(plan),
    };

    json!({
        "snapshot_mode": snapshot_mode_label(generation.snapshot_mode),
        "metadata": {
            "generated_unix_epoch_ms": generation.generated_unix_epoch_ms,
            "source_roots": graph.metadata().source_roots,
            "parser_metadata": {
                "parser_version": graph.metadata().parser_metadata.parser_version,
                "relation_query_version": graph.metadata().parser_metadata.relation_query_version,
            },
            "object_counts": {
                "steps": plan.steps.len(),
            },
        },
        "counts": {
            "total_components": plan.counts.total_components,
            "emitted_steps": plan.counts.emitted_steps,
            "truncated_components": plan.counts.truncated_components,
        },
        "steps": steps,
    })
}

fn collect_graph_nodes(graph: &DependencyGraph, include_details: bool) -> Vec<serde_json::Value> {
    let mut nodes: Vec<_> = graph
        .graph()
        .node_indices()
        .map(|node_index| {
            let node = &graph.graph()[node_index];
            let mut value = json!({
                "node_id": node.node_id,
                "kind": graph_node_kind_label(node.kind),
            });

            if include_details {
                if let serde_json::Value::Object(ref mut map) = value {
                    map.insert(
                        "source_classification".to_owned(),
                        json!(source_classification_label(node.source_classification)),
                    );
                    map.insert(
                        "source".to_owned(),
                        json!(node.source.map(model_source_label)),
                    );
                    map.insert("canonical_id".to_owned(), json!(node.canonical_id));
                    map.insert("model_name".to_owned(), json!(node.model_name));
                    map.insert("symbol_name".to_owned(), json!(node.symbol_name));
                    map.insert(
                        "category".to_owned(),
                        json!(node.category.map(model_category_label)),
                    );
                    map.insert("export_name".to_owned(), json!(node.export_name));
                    map.insert(
                        "definition_path".to_owned(),
                        json!(node.definition_path.as_ref().map(ToString::to_string)),
                    );
                    map.insert("file_path".to_owned(), json!(node.file_path));
                }
            }

            value
        })
        .collect();

    nodes.sort_by_key(|left| value_string(left, "node_id"));
    nodes
}

fn collect_graph_edges(
    graph: &DependencyGraph,
    snapshot_mode: GraphArtifactSnapshotMode,
) -> Vec<serde_json::Value> {
    let mut edges = Vec::new();

    for edge_index in graph.graph().edge_indices() {
        let Some((source_index, target_index)) = graph.graph().edge_endpoints(edge_index) else {
            continue;
        };
        let Some(edge) = graph.graph().edge_weight(edge_index) else {
            continue;
        };

        let source = graph.graph()[source_index].node_id.clone();
        let target = graph.graph()[target_index].node_id.clone();
        let mut value = json!({
            "source_node_id": source,
            "target_node_id": target,
            "kind": edge_kind_label(edge.kind),
            "evidence_count": edge.evidence.len(),
        });

        if let serde_json::Value::Object(ref mut map) = value {
            match snapshot_mode {
                GraphArtifactSnapshotMode::Minimal => {
                    map.insert(
                        "anchor_hashes".to_owned(),
                        json!(collect_edge_anchor_hashes(edge.evidence.iter())),
                    );
                }
                GraphArtifactSnapshotMode::Full => {
                    map.insert(
                        "evidence".to_owned(),
                        json!(collect_relation_evidence(edge.evidence.iter())),
                    );
                }
            }
        }

        edges.push(value);
    }

    edges.sort_by(|left, right| {
        value_string(left, "source_node_id")
            .cmp(&value_string(right, "source_node_id"))
            .then_with(|| {
                value_string(left, "target_node_id").cmp(&value_string(right, "target_node_id"))
            })
            .then_with(|| value_string(left, "kind").cmp(&value_string(right, "kind")))
    });
    edges
}

fn collect_relation_evidence<'a>(
    relations: impl Iterator<Item = &'a ch_core::AstRelationEvidence>,
) -> Vec<serde_json::Value> {
    let mut sorted: Vec<_> = relations.collect();
    sorted.sort_by(|left, right| {
        model_reference_key(&left.source)
            .cmp(&model_reference_key(&right.source))
            .then_with(|| {
                model_reference_key(&left.target).cmp(&model_reference_key(&right.target))
            })
            .then_with(|| edge_kind_label(left.relation).cmp(edge_kind_label(right.relation)))
    });

    sorted
        .into_iter()
        .map(|relation| {
            json!({
                "relation": edge_kind_label(relation.relation),
                "source": relation.source,
                "target": relation.target,
                "anchors": collect_anchor_values(relation.anchors.iter(), true),
            })
        })
        .collect()
}

fn collect_full_mappings(diff: &GraphDiff) -> Vec<serde_json::Value> {
    let mut mappings: Vec<_> = diff.mappings.iter().collect();
    mappings.sort_by(|left, right| {
        left.legacy_canonical_id
            .cmp(&right.legacy_canonical_id)
            .then_with(|| {
                model_reference_key(&left.legacy_symbol)
                    .cmp(&model_reference_key(&right.legacy_symbol))
            })
    });

    mappings
        .into_iter()
        .map(|mapping| {
            json!({
                "legacy_canonical_id": mapping.legacy_canonical_id,
                "legacy_symbol": mapping.legacy_symbol,
                "modern_canonical_id": mapping.modern_canonical_id,
                "modern_symbol": mapping.modern_symbol,
                "confidence_bps": mapping.confidence_bps,
                "status": mapping_status_label(mapping.status),
                "reasons": collect_mapping_reasons(mapping.reasons.iter()),
            })
        })
        .collect()
}

fn collect_mapping_reasons<'a>(
    reasons: impl Iterator<Item = &'a MappingReason>,
) -> Vec<serde_json::Value> {
    let mut sorted: Vec<_> = reasons.collect();
    sorted.sort_by(|left, right| {
        mapping_reason_kind_label(left.kind)
            .cmp(mapping_reason_kind_label(right.kind))
            .then_with(|| left.weight_bps.cmp(&right.weight_bps))
            .then_with(|| left.details.cmp(&right.details))
    });

    sorted
        .into_iter()
        .map(|reason| {
            json!({
                "kind": mapping_reason_kind_label(reason.kind),
                "weight_bps": reason.weight_bps,
                "details": reason.details,
            })
        })
        .collect()
}

fn collect_full_residuals(diff: &GraphDiff) -> Vec<serde_json::Value> {
    let mut residuals: Vec<_> = diff.residual_legacy_usages.iter().collect();
    residuals.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then_with(|| {
                model_reference_key(&left.legacy_symbol)
                    .cmp(&model_reference_key(&right.legacy_symbol))
            })
            .then_with(|| {
                model_reference_key(&left.suggested_modern_symbol)
                    .cmp(&model_reference_key(&right.suggested_modern_symbol))
            })
            .then_with(|| left.confidence_bps.cmp(&right.confidence_bps))
    });

    residuals
        .into_iter()
        .map(|residual| {
            json!({
                "file_path": residual.file_path,
                "legacy_symbol": residual.legacy_symbol,
                "suggested_modern_symbol": residual.suggested_modern_symbol,
                "confidence_bps": residual.confidence_bps,
                "reasons": collect_mapping_reasons(residual.reasons.iter()),
                "anchors": collect_anchor_values(residual.anchors.iter(), true),
            })
        })
        .collect()
}

fn collect_residual_anchor_hashes(diff: &GraphDiff) -> Vec<u64> {
    let mut hashes = collect_edge_anchor_hashes(
        diff.residual_legacy_usages
            .iter()
            .flat_map(|residual| residual.anchors.iter())
            .map(AnchorWrapper::new),
    );
    hashes.sort_unstable();
    hashes.dedup();
    hashes
}

fn collect_edge_anchor_hashes<'a, I, T>(anchors: I) -> Vec<u64>
where
    I: Iterator<Item = T>,
    T: IntoAnchorIterItem<'a>,
{
    let mut hashes = Vec::new();
    for item in anchors {
        for anchor in item.anchors() {
            hashes.push(anchor.snippet_hash);
        }
    }
    hashes.sort_unstable();
    hashes.dedup();
    hashes
}

trait IntoAnchorIterItem<'a> {
    fn anchors(self) -> Box<dyn Iterator<Item = &'a CstAnchor> + 'a>;
}

impl<'a> IntoAnchorIterItem<'a> for &'a ch_core::AstRelationEvidence {
    fn anchors(self) -> Box<dyn Iterator<Item = &'a CstAnchor> + 'a> {
        Box::new(self.anchors.iter())
    }
}

#[derive(Clone, Copy)]
struct AnchorWrapper<'a>(&'a CstAnchor);

impl<'a> AnchorWrapper<'a> {
    const fn new(anchor: &'a CstAnchor) -> Self {
        Self(anchor)
    }
}

impl<'a> IntoAnchorIterItem<'a> for AnchorWrapper<'a> {
    fn anchors(self) -> Box<dyn Iterator<Item = &'a CstAnchor> + 'a> {
        Box::new(std::iter::once(self.0))
    }
}

fn collect_anchor_values<'a>(
    anchors: impl Iterator<Item = &'a CstAnchor>,
    include_details: bool,
) -> Vec<serde_json::Value> {
    let mut sorted: Vec<_> = anchors.collect();
    sorted.sort_by(|left, right| compare_anchor(left, right));

    sorted
        .into_iter()
        .map(|anchor| {
            if include_details {
                json!({
                    "file_path": anchor.file_path,
                    "start_byte": anchor.start_byte,
                    "end_byte": anchor.end_byte,
                    "start": anchor.start,
                    "end": anchor.end,
                    "node_kind": anchor.node_kind,
                    "field_name": anchor.field_name,
                    "import_kind": anchor.import_kind,
                    "is_type_only": anchor.is_type_only,
                    "snippet_hash": anchor.snippet_hash,
                })
            } else {
                json!({
                    "file_path": anchor.file_path,
                    "snippet_hash": anchor.snippet_hash,
                })
            }
        })
        .collect()
}

fn collect_minimal_plan_steps(plan: &MigrationPlan) -> Vec<serde_json::Value> {
    let mut steps: Vec<_> = plan.steps.iter().collect();
    steps.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.step_id.cmp(&right.step_id))
    });

    steps
        .into_iter()
        .map(|step| {
            json!({
                "step_id": step.step_id,
                "order": step.order,
                "component_id": step.component_id,
                "prerequisites": sorted_strings(step.prerequisites.iter().cloned()),
                "risk_score_bps": step.risk_score_bps,
                "impacted_file_count": step.impacted_files.len(),
                "evidence_anchor_hashes": collect_step_anchor_hashes(step),
            })
        })
        .collect()
}

fn collect_full_plan_steps(plan: &MigrationPlan) -> Vec<serde_json::Value> {
    let mut steps: Vec<_> = plan.steps.iter().collect();
    steps.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.step_id.cmp(&right.step_id))
    });

    steps
        .into_iter()
        .map(|step| {
            json!({
                "step_id": step.step_id,
                "order": step.order,
                "component_id": step.component_id,
                "node_ids": sorted_strings(step.node_ids.iter().cloned()),
                "prerequisites": sorted_strings(step.prerequisites.iter().cloned()),
                "impacted_files": sorted_paths(step.impacted_files.iter()),
                "suggested_replacements": collect_suggested_replacements(step.suggested_replacements.iter()),
                "evidence_refs": collect_evidence_refs(step),
                "risk_score_bps": step.risk_score_bps,
                "risk_breakdown": {
                    "components": collect_risk_components(step.risk_breakdown.components.iter()),
                },
            })
        })
        .collect()
}

fn collect_step_anchor_hashes(step: &MigrationStep) -> Vec<u64> {
    let mut hashes = Vec::new();
    for evidence in &step.evidence_refs {
        for anchor in &evidence.anchors {
            hashes.push(anchor.snippet_hash);
        }
    }
    hashes.sort_unstable();
    hashes.dedup();
    hashes
}

fn collect_suggested_replacements<'a>(
    replacements: impl Iterator<Item = &'a SuggestedReplacement>,
) -> Vec<serde_json::Value> {
    let mut sorted: Vec<_> = replacements.collect();
    sorted.sort_by(|left, right| {
        model_reference_key(&left.legacy_symbol)
            .cmp(&model_reference_key(&right.legacy_symbol))
            .then_with(|| {
                model_reference_key(&left.modern_symbol)
                    .cmp(&model_reference_key(&right.modern_symbol))
            })
            .then_with(|| left.confidence_bps.cmp(&right.confidence_bps))
    });

    sorted
        .into_iter()
        .map(|replacement| {
            json!({
                "legacy_symbol": replacement.legacy_symbol,
                "modern_symbol": replacement.modern_symbol,
                "confidence_bps": replacement.confidence_bps,
            })
        })
        .collect()
}

fn collect_evidence_refs(step: &MigrationStep) -> Vec<serde_json::Value> {
    let mut evidence: Vec<_> = step.evidence_refs.iter().collect();
    evidence.sort_by(|left, right| {
        edge_kind_label(left.relation)
            .cmp(edge_kind_label(right.relation))
            .then_with(|| {
                model_reference_key(&left.source).cmp(&model_reference_key(&right.source))
            })
            .then_with(|| {
                model_reference_key(&left.target).cmp(&model_reference_key(&right.target))
            })
    });

    evidence
        .into_iter()
        .map(|evidence| {
            json!({
                "relation": edge_kind_label(evidence.relation),
                "source": evidence.source,
                "target": evidence.target,
                "anchors": collect_anchor_values(evidence.anchors.iter(), true),
            })
        })
        .collect()
}

fn collect_risk_components<'a>(
    components: impl Iterator<Item = &'a RiskComponentScore>,
) -> Vec<serde_json::Value> {
    let mut sorted: Vec<_> = components.collect();
    sorted.sort_by(|left, right| {
        risk_signal_kind_label(left.kind)
            .cmp(risk_signal_kind_label(right.kind))
            .then_with(|| left.raw_value.cmp(&right.raw_value))
            .then_with(|| left.normalized_bps.cmp(&right.normalized_bps))
            .then_with(|| left.weighted_bps.cmp(&right.weighted_bps))
    });

    sorted
        .into_iter()
        .map(|component| {
            json!({
                "kind": risk_signal_kind_label(component.kind),
                "raw_value": component.raw_value,
                "normalized_bps": component.normalized_bps,
                "weighted_bps": component.weighted_bps,
            })
        })
        .collect()
}

fn render_graph_dot(generation: GenerationMetadata, graph: &DependencyGraph) -> String {
    use std::fmt::Write as _;

    let counts = &graph.metadata().counts;
    let mut output = String::new();
    let _ = writeln!(
        output,
        "// generated_unix_epoch_ms: {}",
        generation.generated_unix_epoch_ms
    );
    let _ = writeln!(
        output,
        "// snapshot_mode: {}",
        snapshot_mode_label(generation.snapshot_mode)
    );
    let _ = writeln!(output, "// total_nodes: {}", counts.total_nodes);
    let _ = writeln!(output, "// total_edges: {}", counts.total_edges);
    let _ = writeln!(output, "digraph dependency_graph {{");
    let _ = writeln!(output, "  rankdir=LR;");

    for node in collect_graph_nodes(
        graph,
        generation.snapshot_mode == GraphArtifactSnapshotMode::Full,
    ) {
        let node_id = value_string(&node, "node_id");
        let label = if generation.snapshot_mode == GraphArtifactSnapshotMode::Full {
            format!("{node_id}\\n{}", value_string(&node, "kind"))
        } else {
            node_id.clone()
        };
        let _ = writeln!(
            output,
            "  \"{}\" [label=\"{}\"];",
            dot_escape(&node_id),
            dot_escape(&label)
        );
    }

    for edge in collect_graph_edges(graph, GraphArtifactSnapshotMode::Minimal) {
        let source = value_string(&edge, "source_node_id");
        let target = value_string(&edge, "target_node_id");
        let kind = value_string(&edge, "kind");
        let _ = writeln!(
            output,
            "  \"{}\" -> \"{}\" [label=\"{}\"];",
            dot_escape(&source),
            dot_escape(&target),
            dot_escape(&kind)
        );
    }

    let _ = writeln!(output, "}}");
    output
}

fn render_plan_markdown(
    generation: GenerationMetadata,
    graph: &DependencyGraph,
    diff: &GraphDiff,
    plan: &MigrationPlan,
) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "# Migration Plan");
    let _ = writeln!(output);
    let _ = writeln!(
        output,
        "- Generated unix epoch ms: `{}`",
        generation.generated_unix_epoch_ms
    );
    let _ = writeln!(
        output,
        "- Snapshot mode: `{}`",
        snapshot_mode_label(generation.snapshot_mode)
    );
    let _ = writeln!(
        output,
        "- Parser version: `{}`",
        graph
            .metadata()
            .parser_metadata
            .parser_version
            .as_deref()
            .unwrap_or("unknown")
    );
    let _ = writeln!(
        output,
        "- Relation query version: `{}`",
        graph
            .metadata()
            .parser_metadata
            .relation_query_version
            .as_deref()
            .unwrap_or("unknown")
    );
    let _ = writeln!(
        output,
        "- Source roots: `{}`",
        graph
            .metadata()
            .source_roots
            .iter()
            .map(|root| root.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        output,
        "- Total graph nodes: `{}`",
        graph.metadata().counts.total_nodes
    );
    let _ = writeln!(
        output,
        "- Total graph edges: `{}`",
        graph.metadata().counts.total_edges
    );
    let _ = writeln!(output, "- Mappings matched: `{}`", diff.counts.matched);
    let _ = writeln!(
        output,
        "- Mappings low-confidence: `{}`",
        diff.counts.low_confidence
    );
    let _ = writeln!(output, "- Mappings no-match: `{}`", diff.counts.no_match);
    let _ = writeln!(
        output,
        "- Residual legacy usages: `{}`",
        diff.counts.residuals
    );
    let _ = writeln!(
        output,
        "- Total components: `{}`",
        plan.counts.total_components
    );
    let _ = writeln!(output, "- Emitted steps: `{}`", plan.counts.emitted_steps);
    let _ = writeln!(
        output,
        "- Truncated components: `{}`",
        plan.counts.truncated_components
    );
    let _ = writeln!(output);
    let _ = writeln!(output, "| Step | Order | Prerequisites | Risk (bps) |");
    let _ = writeln!(output, "| --- | --- | --- | --- |");

    let mut steps: Vec<_> = plan.steps.iter().collect();
    steps.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.step_id.cmp(&right.step_id))
    });
    for step in steps {
        let prerequisites = if step.prerequisites.is_empty() {
            "-".to_owned()
        } else {
            let mut sorted = step.prerequisites.clone();
            sorted.sort();
            sorted.join(", ")
        };
        let _ = writeln!(
            output,
            "| `{}` | `{}` | `{}` | `{}` |",
            step.step_id, step.order, prerequisites, step.risk_score_bps
        );
    }

    if generation.snapshot_mode == GraphArtifactSnapshotMode::Full {
        for step in &plan.steps {
            let _ = writeln!(output);
            let _ = writeln!(output, "## {}", step.step_id);
            let _ = writeln!(output);
            let _ = writeln!(output, "- Component: `{}`", step.component_id);
            let _ = writeln!(output, "- Node count: `{}`", step.node_ids.len());
            let _ = writeln!(output, "- Impacted files: `{}`", step.impacted_files.len());
            let _ = writeln!(
                output,
                "- Suggested replacements: `{}`",
                step.suggested_replacements.len()
            );
            let _ = writeln!(output, "- Evidence refs: `{}`", step.evidence_refs.len());
        }
    }

    output
}

fn compare_anchor(left: &CstAnchor, right: &CstAnchor) -> std::cmp::Ordering {
    left.file_path
        .cmp(&right.file_path)
        .then_with(|| left.start_byte.cmp(&right.start_byte))
        .then_with(|| left.end_byte.cmp(&right.end_byte))
        .then_with(|| left.start.line.cmp(&right.start.line))
        .then_with(|| left.start.column.cmp(&right.start.column))
        .then_with(|| left.end.line.cmp(&right.end.line))
        .then_with(|| left.end.column.cmp(&right.end.column))
        .then_with(|| left.node_kind.cmp(&right.node_kind))
        .then_with(|| left.field_name.cmp(&right.field_name))
        .then_with(|| import_kind_label(left.import_kind).cmp(import_kind_label(right.import_kind)))
        .then_with(|| left.is_type_only.cmp(&right.is_type_only))
        .then_with(|| left.snippet_hash.cmp(&right.snippet_hash))
}

fn sorted_strings<I>(values: I) -> Vec<String>
where
    I: Iterator<Item = String>,
{
    let mut sorted: Vec<_> = values.collect();
    sorted.sort();
    sorted
}

fn sorted_paths<'a>(paths: impl Iterator<Item = &'a Utf8PathBuf>) -> Vec<String> {
    let mut sorted: Vec<_> = paths.map(ToString::to_string).collect();
    sorted.sort();
    sorted
}

fn model_reference_key(reference: &ch_core::ModelReference) -> (String, String, String) {
    (
        model_source_label(reference.source).to_owned(),
        model_category_label(reference.category).to_owned(),
        reference.name.clone(),
    )
}

fn dot_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn value_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_default()
}

fn value_usize(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|raw| usize::try_from(raw).ok())
        .unwrap_or_default()
}

const fn snapshot_mode_label(mode: GraphArtifactSnapshotMode) -> &'static str {
    match mode {
        GraphArtifactSnapshotMode::Minimal => "minimal",
        GraphArtifactSnapshotMode::Full => "full",
    }
}

const fn graph_node_kind_label(kind: GraphNodeKind) -> &'static str {
    match kind {
        GraphNodeKind::Interface => "interface",
        GraphNodeKind::Model => "model",
        GraphNodeKind::Service => "service",
        GraphNodeKind::File => "file",
        GraphNodeKind::Symbol => "symbol",
    }
}

const fn source_classification_label(
    classification: ch_core::SourceClassification,
) -> &'static str {
    match classification {
        ch_core::SourceClassification::Legacy => "legacy",
        ch_core::SourceClassification::Modern => "modern",
        _ => "unknown",
    }
}

const fn import_kind_label(kind: Option<ch_core::ImportKind>) -> &'static str {
    match kind {
        Some(ch_core::ImportKind::Named) => "named",
        Some(ch_core::ImportKind::Default) => "default",
        Some(ch_core::ImportKind::Namespace) => "namespace",
        Some(ch_core::ImportKind::SideEffect) => "side_effect",
        Some(ch_core::ImportKind::TypeOnly) => "type_only",
        Some(ch_core::ImportKind::Dynamic) => "dynamic",
        _ => "none",
    }
}

const fn model_source_label(source: ch_core::ModelSource) -> &'static str {
    match source {
        ch_core::ModelSource::SharedLegacy => "shared_legacy",
        ch_core::ModelSource::Shared2023 => "shared_2023",
        _ => "unknown",
    }
}

const fn model_category_label(category: ch_core::ModelCategory) -> &'static str {
    match category {
        ch_core::ModelCategory::Interface => "interface",
        ch_core::ModelCategory::Model => "model",
        ch_core::ModelCategory::Service => "service",
        ch_core::ModelCategory::CodeGen => "code_gen",
        ch_core::ModelCategory::CodeGenForApi => "code_gen_for_api",
        ch_core::ModelCategory::CodeGenForm => "code_gen_form",
        ch_core::ModelCategory::CodeGenFormArray => "code_gen_form_array",
        ch_core::ModelCategory::ServiceCodeGen => "service_code_gen",
        _ => "unknown",
    }
}

const fn edge_kind_label(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::ImportsSymbol => "imports_symbol",
        EdgeKind::ReexportsSymbol => "reexports_symbol",
        EdgeKind::TypeRef => "type_ref",
        EdgeKind::Extends => "extends",
        EdgeKind::Implements => "implements",
        EdgeKind::Constructs => "constructs",
        EdgeKind::FactoryCall => "factory_call",
        EdgeKind::ServiceParamType => "service_param_type",
        EdgeKind::ServiceReturnType => "service_return_type",
        EdgeKind::ModelMapRegistration => "model_map_registration",
        EdgeKind::LegacyBridge => "legacy_bridge",
        EdgeKind::GeneratedFrom => "generated_from",
        _ => "unknown",
    }
}

const fn mapping_status_label(status: MappingStatus) -> &'static str {
    match status {
        MappingStatus::Matched => "matched",
        MappingStatus::LowConfidence => "low_confidence",
        MappingStatus::NoMatch => "no_match",
    }
}

const fn mapping_reason_kind_label(kind: MappingReasonKind) -> &'static str {
    match kind {
        MappingReasonKind::CanonicalIdExact => "canonical_id_exact",
        MappingReasonKind::InterfaceNameEquivalent => "interface_name_equivalent",
        MappingReasonKind::ExportOverlap => "export_overlap",
        MappingReasonKind::UsageNeighborhoodOverlap => "usage_neighborhood_overlap",
    }
}

const fn risk_signal_kind_label(kind: RiskSignalKind) -> &'static str {
    match kind {
        RiskSignalKind::DownstreamFanout => "downstream_fanout",
        RiskSignalKind::SccSize => "scc_size",
        RiskSignalKind::MixedDependencyCount => "mixed_dependency_count",
        RiskSignalKind::ServiceSurfaceCoupling => "service_surface_coupling",
        RiskSignalKind::UnresolvedMappingPenalty => "unresolved_mapping_penalty",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use ch_core::{
        AstRelationEvidence, CstAnchor, EdgeKind, ModelCategory, ModelReference, ModelSource,
        SourceLocation,
    };
    use smallvec::smallvec;

    use crate::graph::{
        DependencyGraph, DependencyStableGraph, GraphCounts, GraphEdge, GraphEdgeKindCount,
        GraphMetadata, GraphNode, ParserMetadata,
    };
    use crate::mapping::{
        GraphDiff, LegacyResidual, MappingReason, MappingReasonKind, MappingStatus, ModelMapping,
    };
    use crate::planner::{
        EvidenceRef, MigrationPlan, MigrationStep, RiskBreakdown, RiskComponentScore,
        RiskSignalKind, SuggestedReplacement,
    };

    #[test]
    fn test_export_artifacts_all_writes_contract_files() {
        let (graph, diff, plan) = sample_payload();
        let output_dir = temp_export_dir("all-contract");
        let create_result = std::fs::create_dir_all(output_dir.as_std_path());
        assert!(create_result.is_ok());

        let export_result = export_artifacts(
            output_dir.as_path(),
            GraphArtifactSnapshotMode::Minimal,
            GraphArtifactFormat::All,
            &graph,
            &diff,
            &plan,
        );
        assert!(export_result.is_ok());

        if let Ok(written) = export_result {
            let mut names: Vec<_> = written
                .iter()
                .filter_map(|path| path.file_name().map(ToOwned::to_owned))
                .collect();
            names.sort();
            assert_eq!(
                names,
                vec![
                    "graph.dot".to_owned(),
                    "graph.json".to_owned(),
                    "migration-plan.json".to_owned(),
                    "migration-plan.md".to_owned()
                ]
            );
        }

        let _ = std::fs::remove_dir_all(output_dir.as_std_path());
    }

    #[test]
    fn test_export_snapshot_modes_produce_distinct_json_shapes() {
        let (graph, diff, plan) = sample_payload();
        let minimal_dir = temp_export_dir("minimal-shape");
        let full_dir = temp_export_dir("full-shape");
        let _ = std::fs::create_dir_all(minimal_dir.as_std_path());
        let _ = std::fs::create_dir_all(full_dir.as_std_path());

        let minimal_result = export_artifacts(
            minimal_dir.as_path(),
            GraphArtifactSnapshotMode::Minimal,
            GraphArtifactFormat::Json,
            &graph,
            &diff,
            &plan,
        );
        assert!(minimal_result.is_ok());

        let full_result = export_artifacts(
            full_dir.as_path(),
            GraphArtifactSnapshotMode::Full,
            GraphArtifactFormat::Json,
            &graph,
            &diff,
            &plan,
        );
        assert!(full_result.is_ok());

        let minimal_graph_json = read_json(minimal_dir.join("graph.json").as_path());
        let full_graph_json = read_json(full_dir.join("graph.json").as_path());
        assert!(minimal_graph_json.is_some());
        assert!(full_graph_json.is_some());

        if let Some(minimal_graph_json) = minimal_graph_json {
            let graph_section = minimal_graph_json.get("graph");
            assert!(graph_section.is_some());
            if let Some(graph_section) = graph_section {
                assert!(graph_section.get("node_ids").is_some());
                assert!(graph_section.get("nodes").is_none());
            }
        }

        if let Some(full_graph_json) = full_graph_json {
            let graph_section = full_graph_json.get("graph");
            assert!(graph_section.is_some());
            if let Some(graph_section) = graph_section {
                assert!(graph_section.get("nodes").is_some());
                assert!(graph_section.get("node_ids").is_none());
            }
        }

        let _ = std::fs::remove_dir_all(minimal_dir.as_std_path());
        let _ = std::fs::remove_dir_all(full_dir.as_std_path());
    }

    #[test]
    fn test_export_outputs_are_deterministic_excluding_timestamp() {
        let (graph, diff, plan) = sample_payload();
        let first_dir = temp_export_dir("determinism-a");
        let second_dir = temp_export_dir("determinism-b");
        let _ = std::fs::create_dir_all(first_dir.as_std_path());
        let _ = std::fs::create_dir_all(second_dir.as_std_path());

        let first_result = export_artifacts(
            first_dir.as_path(),
            GraphArtifactSnapshotMode::Full,
            GraphArtifactFormat::All,
            &graph,
            &diff,
            &plan,
        );
        assert!(first_result.is_ok());

        let second_result = export_artifacts(
            second_dir.as_path(),
            GraphArtifactSnapshotMode::Full,
            GraphArtifactFormat::All,
            &graph,
            &diff,
            &plan,
        );
        assert!(second_result.is_ok());

        let first_graph_json = read_json(first_dir.join("graph.json").as_path());
        let second_graph_json = read_json(second_dir.join("graph.json").as_path());
        assert!(first_graph_json.is_some());
        assert!(second_graph_json.is_some());

        if let (Some(mut first_graph_json), Some(mut second_graph_json)) =
            (first_graph_json, second_graph_json)
        {
            strip_generated_timestamps(&mut first_graph_json);
            strip_generated_timestamps(&mut second_graph_json);
            assert_eq!(first_graph_json, second_graph_json);
        }

        let first_plan_json = read_json(first_dir.join("migration-plan.json").as_path());
        let second_plan_json = read_json(second_dir.join("migration-plan.json").as_path());
        assert!(first_plan_json.is_some());
        assert!(second_plan_json.is_some());

        if let (Some(mut first_plan_json), Some(mut second_plan_json)) =
            (first_plan_json, second_plan_json)
        {
            strip_generated_timestamps(&mut first_plan_json);
            strip_generated_timestamps(&mut second_plan_json);
            assert_eq!(first_plan_json, second_plan_json);
        }

        let first_dot = std::fs::read_to_string(first_dir.join("graph.dot").as_std_path());
        let second_dot = std::fs::read_to_string(second_dir.join("graph.dot").as_std_path());
        assert!(first_dot.is_ok());
        assert!(second_dot.is_ok());
        if let (Ok(first_dot), Ok(second_dot)) = (first_dot, second_dot) {
            assert_eq!(normalize_dot(&first_dot), normalize_dot(&second_dot));
        }

        let first_markdown =
            std::fs::read_to_string(first_dir.join("migration-plan.md").as_std_path());
        let second_markdown =
            std::fs::read_to_string(second_dir.join("migration-plan.md").as_std_path());
        assert!(first_markdown.is_ok());
        assert!(second_markdown.is_ok());
        if let (Ok(first_markdown), Ok(second_markdown)) = (first_markdown, second_markdown) {
            assert_eq!(
                normalize_markdown(&first_markdown),
                normalize_markdown(&second_markdown)
            );
        }

        let _ = std::fs::remove_dir_all(first_dir.as_std_path());
        let _ = std::fs::remove_dir_all(second_dir.as_std_path());
    }

    fn sample_payload() -> (DependencyGraph, GraphDiff, MigrationPlan) {
        let mut graph = DependencyStableGraph::default();
        let file_node = graph.add_node(GraphNode::file(
            "file:app/features/order-consumer.ts".to_owned(),
            Utf8PathBuf::from("app/features/order-consumer.ts"),
        ));
        let symbol_node = graph.add_node(GraphNode::symbol(
            "symbol:legacy:OrderModel".to_owned(),
            ModelSource::SharedLegacy,
            "OrderModel".to_owned(),
            ModelCategory::Model,
        ));

        let mut relation = AstRelationEvidence::new(
            EdgeKind::ImportsSymbol,
            ModelReference::new(
                "OrderModel",
                ModelCategory::Model,
                ModelSource::SharedLegacy,
            ),
            ModelReference::new(
                "OrderModel",
                ModelCategory::Model,
                ModelSource::SharedLegacy,
            ),
        );
        relation.add_anchor(sample_anchor("app/features/order-consumer.ts", 101));
        graph.add_edge(
            file_node,
            symbol_node,
            GraphEdge::new(EdgeKind::ImportsSymbol, smallvec![relation.clone()]),
        );

        let metadata = GraphMetadata {
            counts: GraphCounts {
                total_nodes: graph.node_count(),
                total_edges: graph.edge_count(),
                interface_nodes: 0,
                model_nodes: 0,
                service_nodes: 0,
                file_nodes: 1,
                symbol_nodes: 1,
                edges_by_kind: vec![GraphEdgeKindCount {
                    kind: EdgeKind::ImportsSymbol,
                    count: 1,
                }],
            },
            source_roots: vec![
                Utf8PathBuf::from("app"),
                Utf8PathBuf::from("app/shared"),
                Utf8PathBuf::from("app/shared_2023"),
            ],
            parser_metadata: ParserMetadata {
                parser_version: Some("tree-sitter-0.24".to_owned()),
                relation_query_version: Some("v1".to_owned()),
            },
        };
        let dependency_graph = DependencyGraph::new(graph, metadata);

        let mapping = ModelMapping {
            legacy_canonical_id: "order".to_owned(),
            legacy_symbol: ModelReference::new(
                "OrderModel",
                ModelCategory::Model,
                ModelSource::SharedLegacy,
            ),
            modern_canonical_id: Some("order".to_owned()),
            modern_symbol: Some(ModelReference::new(
                "OrderModel",
                ModelCategory::Model,
                ModelSource::Shared2023,
            )),
            confidence_bps: 900,
            status: MappingStatus::Matched,
            reasons: smallvec![MappingReason::new(
                MappingReasonKind::CanonicalIdExact,
                450,
                "canonical_id=order",
            )],
        };
        let residual = LegacyResidual {
            file_path: Utf8PathBuf::from("app/features/order-consumer.ts"),
            legacy_symbol: ModelReference::new(
                "OrderModel",
                ModelCategory::Model,
                ModelSource::SharedLegacy,
            ),
            suggested_modern_symbol: ModelReference::new(
                "OrderModel",
                ModelCategory::Model,
                ModelSource::Shared2023,
            ),
            confidence_bps: 900,
            reasons: smallvec![MappingReason::new(
                MappingReasonKind::CanonicalIdExact,
                450,
                "canonical_id=order",
            )],
            anchors: smallvec![sample_anchor("app/features/order-consumer.ts", 101)],
        };
        let diff = GraphDiff::new(vec![mapping], vec![residual]);

        let step = MigrationStep {
            step_id: "step-0001".to_owned(),
            order: 1,
            component_id: "component-order".to_owned(),
            node_ids: vec!["symbol:legacy:OrderModel".to_owned()],
            prerequisites: Vec::new(),
            impacted_files: vec![Utf8PathBuf::from("app/features/order-consumer.ts")],
            suggested_replacements: vec![SuggestedReplacement {
                legacy_symbol: ModelReference::new(
                    "OrderModel",
                    ModelCategory::Model,
                    ModelSource::SharedLegacy,
                ),
                modern_symbol: ModelReference::new(
                    "OrderModel",
                    ModelCategory::Model,
                    ModelSource::Shared2023,
                ),
                confidence_bps: 900,
            }],
            evidence_refs: vec![EvidenceRef {
                relation: EdgeKind::ImportsSymbol,
                source: ModelReference::new(
                    "OrderModel",
                    ModelCategory::Model,
                    ModelSource::SharedLegacy,
                ),
                target: ModelReference::new(
                    "OrderModel",
                    ModelCategory::Model,
                    ModelSource::Shared2023,
                ),
                anchors: smallvec![sample_anchor("app/features/order-consumer.ts", 101)],
            }],
            risk_score_bps: 250,
            risk_breakdown: RiskBreakdown {
                components: smallvec![RiskComponentScore {
                    kind: RiskSignalKind::SccSize,
                    raw_value: 1,
                    normalized_bps: 200,
                    weighted_bps: 40,
                }],
            },
        };
        let plan = MigrationPlan::new(vec![step], 1, 0);
        (dependency_graph, diff, plan)
    }

    fn sample_anchor(path: &str, snippet_hash: u64) -> CstAnchor {
        let mut anchor = CstAnchor::new(
            Utf8PathBuf::from(path),
            0,
            10,
            SourceLocation::new(1, 0, 0),
            SourceLocation::new(1, 10, 10),
            "identifier",
        );
        anchor.snippet_hash = snippet_hash;
        anchor
    }

    fn temp_export_dir(suffix: &str) -> Utf8PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let raw = std::env::temp_dir().join(format!("ch-graph-export-{suffix}-{nanos}"));
        Utf8PathBuf::from_str(raw.to_string_lossy().as_ref())
            .unwrap_or_else(|_| Utf8PathBuf::from(format!("/tmp/ch-graph-export-{suffix}-{nanos}")))
    }

    fn read_json(path: &Utf8Path) -> Option<serde_json::Value> {
        let raw = std::fs::read_to_string(path.as_std_path()).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn strip_generated_timestamps(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("generated_unix_epoch_ms");
                for nested in map.values_mut() {
                    strip_generated_timestamps(nested);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    strip_generated_timestamps(item);
                }
            }
            _ => {}
        }
    }

    fn normalize_dot(dot: &str) -> String {
        dot.lines()
            .filter(|line| !line.starts_with("// generated_unix_epoch_ms:"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn normalize_markdown(markdown: &str) -> String {
        markdown
            .lines()
            .filter(|line| !line.starts_with("- Generated unix epoch ms:"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
