//! Deterministic migration planner built on SCC condensation.
//!
//! The planner consumes a [`crate::graph::DependencyGraph`] and comparator
//! output to produce an ordered migration plan with explicit prerequisites and
//! explainable risk scoring.

use std::cmp::Ordering;

use camino::Utf8PathBuf;
use ch_core::{
    CstAnchor, EdgeKind, FxHashMap, FxHashSet, ModelCategory, ModelReference, ModelSource,
    SourceClassification,
};
use ch_ts_parser::pascal_to_kebab;
use petgraph::algo::{condensation, tarjan_scc};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use smallvec::SmallVec;

use crate::graph::{DependencyGraph, GraphNode, GraphNodeKind};
use crate::mapping::{GraphDiff, MappingStatus, MAX_CONFIDENCE_BPS};

/// Maximum risk score represented in basis points (`1000 == 100%`).
pub const MAX_RISK_BPS: u16 = MAX_CONFIDENCE_BPS;

/// Weights for planner risk components in basis points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannerRiskWeights {
    /// Weight for downstream fanout contribution.
    pub downstream_fanout_bps: u16,
    /// Weight for SCC size contribution.
    pub scc_size_bps: u16,
    /// Weight for mixed legacy/modern dependency contribution.
    pub mixed_dependency_bps: u16,
    /// Weight for service-surface coupling contribution.
    pub service_coupling_bps: u16,
    /// Weight for unresolved mapping penalty contribution.
    pub unresolved_mapping_bps: u16,
}

impl Default for PlannerRiskWeights {
    fn default() -> Self {
        Self {
            downstream_fanout_bps: 240,
            scc_size_bps: 200,
            mixed_dependency_bps: 180,
            service_coupling_bps: 120,
            unresolved_mapping_bps: 260,
        }
    }
}

impl PlannerRiskWeights {
    /// Returns the total configured weight in basis points.
    #[must_use]
    pub const fn total_bps(self) -> u16 {
        self.downstream_fanout_bps
            .saturating_add(self.scc_size_bps)
            .saturating_add(self.mixed_dependency_bps)
            .saturating_add(self.service_coupling_bps)
            .saturating_add(self.unresolved_mapping_bps)
    }
}

/// Planner configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerConfig {
    /// Optional step cap applied after deterministic ordering.
    pub max_steps: Option<usize>,
    /// Weight set used to score component risk.
    pub risk_weights: PlannerRiskWeights,
}

/// Planner risk signal class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RiskSignalKind {
    /// Unique outgoing condensed dependencies.
    DownstreamFanout,
    /// Size of the SCC/component.
    SccSize,
    /// Number of mixed legacy/modern dependencies touching the component.
    MixedDependencyCount,
    /// Service-related coupling touching the component.
    ServiceSurfaceCoupling,
    /// Unresolved mapping and residual penalty.
    UnresolvedMappingPenalty,
}

/// One explainable risk component score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskComponentScore {
    /// Risk signal class.
    pub kind: RiskSignalKind,
    /// Raw unbounded value for this signal.
    pub raw_value: usize,
    /// Signal normalized to basis points against plan-wide maxima.
    pub normalized_bps: u16,
    /// Weighted contribution in basis points.
    pub weighted_bps: u16,
}

/// Risk breakdown for one migration step.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RiskBreakdown {
    /// Ordered component scores used to derive total risk.
    pub components: SmallVec<[RiskComponentScore; 5]>,
}

/// Suggested legacy-to-modern replacement for one symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestedReplacement {
    /// Legacy symbol still requiring migration.
    pub legacy_symbol: ModelReference,
    /// Suggested modern equivalent symbol.
    pub modern_symbol: ModelReference,
    /// Confidence in basis points from comparator output.
    pub confidence_bps: u16,
}

/// Evidence pointer for a migration step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRef {
    /// Relation semantics.
    pub relation: EdgeKind,
    /// Source symbol reference.
    pub source: ModelReference,
    /// Target symbol reference.
    pub target: ModelReference,
    /// Supporting anchors.
    pub anchors: SmallVec<[CstAnchor; 2]>,
}

/// One deterministic migration step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStep {
    /// Stable step identifier (`step-0001`, ...).
    pub step_id: String,
    /// 1-based execution order.
    pub order: usize,
    /// Stable component identifier.
    pub component_id: String,
    /// Sorted member node IDs for this component.
    pub node_ids: Vec<String>,
    /// Sorted prerequisite step IDs.
    pub prerequisites: Vec<String>,
    /// Deterministically ordered impacted files.
    pub impacted_files: Vec<Utf8PathBuf>,
    /// Suggested symbol replacements for this step.
    pub suggested_replacements: Vec<SuggestedReplacement>,
    /// Evidence references justifying this step.
    pub evidence_refs: Vec<EvidenceRef>,
    /// Total risk score in basis points.
    pub risk_score_bps: u16,
    /// Explainable per-signal risk contributions.
    pub risk_breakdown: RiskBreakdown,
}

/// Summary counts for a migration plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MigrationPlanCounts {
    /// Number of condensed components before truncation.
    pub total_components: usize,
    /// Number of emitted steps after optional truncation.
    pub emitted_steps: usize,
    /// Number of truncated components.
    pub truncated_components: usize,
}

/// Full deterministic migration plan output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MigrationPlan {
    /// Ordered migration steps.
    pub steps: Vec<MigrationStep>,
    /// Derived summary counts.
    pub counts: MigrationPlanCounts,
}

impl MigrationPlan {
    /// Creates a migration plan and derives summary counts.
    #[must_use]
    pub fn new(
        steps: Vec<MigrationStep>,
        total_components: usize,
        truncated_components: usize,
    ) -> Self {
        Self {
            counts: MigrationPlanCounts {
                total_components,
                emitted_steps: steps.len(),
                truncated_components,
            },
            steps,
        }
    }

    /// Returns `true` when no steps were emitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Deterministic migration planner.
#[derive(Debug, Clone, Default)]
pub struct GraphPlanner {
    config: PlannerConfig,
}

impl GraphPlanner {
    /// Creates a planner using [`PlannerConfig::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a planner with explicit config.
    #[must_use]
    pub const fn with_config(config: PlannerConfig) -> Self {
        Self { config }
    }

    /// Returns the active planner config.
    #[must_use]
    pub const fn config(&self) -> &PlannerConfig {
        &self.config
    }

    /// Produces a deterministic migration plan from graph and comparator output.
    #[must_use]
    pub fn plan(&self, graph: &DependencyGraph, diff: &GraphDiff) -> MigrationPlan {
        let Some(symbol_graph) = build_symbol_graph(graph) else {
            return MigrationPlan::default();
        };

        let mut components = build_components(&symbol_graph);
        if components.is_empty() {
            return MigrationPlan::default();
        }

        let node_to_component = build_node_to_component(&components);
        populate_component_evidence_and_metrics(&mut components, graph, &node_to_component);
        populate_component_diff_signals(&mut components, diff);
        score_component_risk(&mut components, self.config.risk_weights);

        let ordered_components = deterministic_topo_order(&components);
        build_plan_steps(&components, &ordered_components, self.config.max_steps)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SymbolSignature {
    source: ModelSource,
    category: ModelCategory,
    name: String,
}

impl SymbolSignature {
    fn from_model_reference(model_ref: &ModelReference) -> Self {
        Self {
            source: model_ref.source,
            category: model_ref.category,
            name: model_ref.name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct SymbolNodeData {
    original_index: NodeIndex<u32>,
    node_id: String,
    category: ModelCategory,
    symbol_ref: ModelReference,
    legacy_canonical_id_hint: Option<String>,
}

impl SymbolNodeData {
    fn from_graph_node(original_index: NodeIndex<u32>, node: &GraphNode) -> Option<Self> {
        if node.kind != GraphNodeKind::Symbol {
            return None;
        }

        let source = node.source?;
        let category = node.category?;
        let symbol_name = node.symbol_name.as_ref()?;
        let symbol_ref = ModelReference::new(symbol_name.clone(), category, source);

        let legacy_canonical_id_hint = if source.is_legacy() {
            Some(pascal_to_kebab(symbol_stem(symbol_name)))
        } else {
            None
        };

        Some(Self {
            original_index,
            node_id: node.node_id.clone(),
            category,
            symbol_ref,
            legacy_canonical_id_hint,
        })
    }
}

#[derive(Debug, Clone)]
struct ComponentData {
    component_id: String,
    component_key: String,
    node_ids: Vec<String>,
    member_indices: Vec<NodeIndex<u32>>,
    predecessors: Vec<usize>,
    successors: Vec<usize>,
    symbol_signatures: FxHashSet<SymbolSignature>,
    legacy_canonical_ids: FxHashSet<String>,
    evidence_refs: Vec<EvidenceRef>,
    impacted_files: Vec<Utf8PathBuf>,
    suggested_replacements: Vec<SuggestedReplacement>,
    scc_size: usize,
    downstream_fanout: usize,
    mixed_dependency_count: usize,
    service_coupling_count: usize,
    unresolved_mapping_count: usize,
    risk_score_bps: u16,
    risk_breakdown: RiskBreakdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EvidenceSignature {
    relation: EdgeKind,
    source: ModelReference,
    target: ModelReference,
    anchors: Vec<CstAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReplacementSignature {
    legacy: SymbolSignature,
    modern: SymbolSignature,
}

#[derive(Debug, Clone, Copy, Default)]
struct SignalMaxima {
    downstream_fanout: usize,
    scc_size: usize,
    mixed_dependency_count: usize,
    service_coupling_count: usize,
    unresolved_mapping_count: usize,
}

fn build_symbol_graph(graph: &DependencyGraph) -> Option<DiGraph<SymbolNodeData, (), u32>> {
    let mut symbol_graph = DiGraph::default();
    let mut node_map: FxHashMap<NodeIndex<u32>, NodeIndex<u32>> = FxHashMap::default();

    let mut symbol_nodes: Vec<(NodeIndex<u32>, SymbolNodeData)> = graph
        .graph()
        .node_indices()
        .filter_map(|node_index| {
            SymbolNodeData::from_graph_node(node_index, &graph.graph()[node_index])
                .map(|node| (node_index, node))
        })
        .collect();
    symbol_nodes.sort_by(|(_, left), (_, right)| left.node_id.cmp(&right.node_id));

    for (original, node) in symbol_nodes {
        let local = symbol_graph.add_node(node);
        node_map.insert(original, local);
    }

    if symbol_graph.node_count() == 0 {
        return None;
    }

    let mut edge_keys: FxHashSet<(usize, usize)> = FxHashSet::default();
    for edge_index in graph.graph().edge_indices() {
        let Some((source, target)) = graph.graph().edge_endpoints(edge_index) else {
            continue;
        };
        let Some(&source_local) = node_map.get(&source) else {
            continue;
        };
        let Some(&target_local) = node_map.get(&target) else {
            continue;
        };

        if edge_keys.insert((source_local.index(), target_local.index())) {
            symbol_graph.add_edge(source_local, target_local, ());
        }
    }

    Some(symbol_graph)
}

fn build_components(symbol_graph: &DiGraph<SymbolNodeData, (), u32>) -> Vec<ComponentData> {
    // Compute SCCs explicitly and then condense to an acyclic DAG.
    let _scc_count = tarjan_scc(symbol_graph).len();
    let condensed = condensation(symbol_graph.clone(), true);
    if condensed.node_count() == 0 {
        return Vec::new();
    }

    let mut raw_components: Vec<(NodeIndex<u32>, Vec<SymbolNodeData>, String)> = condensed
        .node_indices()
        .filter_map(|node_index| {
            let members = condensed[node_index].clone();
            if members.is_empty() {
                return None;
            }

            let mut members = members;
            members.sort_by(|left, right| left.node_id.cmp(&right.node_id));
            let component_key = members
                .iter()
                .map(|node| node.node_id.as_str())
                .collect::<Vec<_>>()
                .join("|");
            Some((node_index, members, component_key))
        })
        .collect();
    raw_components.sort_by(|(_, _, left_key), (_, _, right_key)| left_key.cmp(right_key));

    let mut condensed_to_stable: FxHashMap<NodeIndex<u32>, usize> = FxHashMap::default();
    let mut components = Vec::with_capacity(raw_components.len());

    for (stable_index, (condensed_index, members, component_key)) in
        raw_components.into_iter().enumerate()
    {
        condensed_to_stable.insert(condensed_index, stable_index);

        let mut node_ids = Vec::with_capacity(members.len());
        let mut member_indices = Vec::with_capacity(members.len());
        let mut symbol_signatures: FxHashSet<SymbolSignature> = FxHashSet::default();
        let mut legacy_canonical_ids: FxHashSet<String> = FxHashSet::default();
        let mut service_coupling_count = 0;

        for member in members {
            node_ids.push(member.node_id);
            member_indices.push(member.original_index);
            symbol_signatures.insert(SymbolSignature::from_model_reference(&member.symbol_ref));
            if let Some(canonical_id) = member.legacy_canonical_id_hint {
                legacy_canonical_ids.insert(canonical_id);
            }
            if member.category == ModelCategory::Service
                || member.category == ModelCategory::ServiceCodeGen
            {
                service_coupling_count += 1;
            }
        }
        let scc_size = node_ids.len();

        components.push(ComponentData {
            component_id: format!("component-{index:04}", index = stable_index + 1),
            component_key,
            node_ids,
            member_indices,
            predecessors: Vec::new(),
            successors: Vec::new(),
            symbol_signatures,
            legacy_canonical_ids,
            evidence_refs: Vec::new(),
            impacted_files: Vec::new(),
            suggested_replacements: Vec::new(),
            scc_size,
            downstream_fanout: 0,
            mixed_dependency_count: 0,
            service_coupling_count,
            unresolved_mapping_count: 0,
            risk_score_bps: 0,
            risk_breakdown: RiskBreakdown::default(),
        });
    }

    let mut edge_keys: FxHashSet<(usize, usize)> = FxHashSet::default();
    for edge in condensed.edge_references() {
        let Some(&source) = condensed_to_stable.get(&edge.source()) else {
            continue;
        };
        let Some(&target) = condensed_to_stable.get(&edge.target()) else {
            continue;
        };
        if source == target {
            continue;
        }
        if edge_keys.insert((source, target)) {
            components[source].successors.push(target);
            components[target].predecessors.push(source);
        }
    }

    for component in &mut components {
        component.predecessors.sort_unstable();
        component.predecessors.dedup();
        component.successors.sort_unstable();
        component.successors.dedup();
        component.downstream_fanout = component.successors.len();
    }

    components
}

fn build_node_to_component(components: &[ComponentData]) -> FxHashMap<NodeIndex<u32>, usize> {
    let mut map: FxHashMap<NodeIndex<u32>, usize> = FxHashMap::default();
    for (component_index, component) in components.iter().enumerate() {
        for node_index in &component.member_indices {
            map.insert(*node_index, component_index);
        }
    }
    map
}

fn populate_component_evidence_and_metrics(
    components: &mut [ComponentData],
    graph: &DependencyGraph,
    node_to_component: &FxHashMap<NodeIndex<u32>, usize>,
) {
    let mut evidence_seen: Vec<FxHashSet<EvidenceSignature>> = (0..components.len())
        .map(|_| FxHashSet::default())
        .collect();
    let mut impacted_file_sets: Vec<FxHashSet<Utf8PathBuf>> = (0..components.len())
        .map(|_| FxHashSet::default())
        .collect();

    for edge_index in graph.graph().edge_indices() {
        let Some((source_index, target_index)) = graph.graph().edge_endpoints(edge_index) else {
            continue;
        };
        let Some(&source_component) = node_to_component.get(&source_index) else {
            continue;
        };
        let Some(&target_component) = node_to_component.get(&target_index) else {
            continue;
        };
        let Some(edge) = graph.graph().edge_weight(edge_index) else {
            continue;
        };

        let source_node = &graph.graph()[source_index];
        let target_node = &graph.graph()[target_index];
        let mixed = is_mixed_legacy_modern(
            source_node.source_classification,
            target_node.source_classification,
        );
        let service_edge = is_service_edge_kind(edge.kind)
            || is_service_category(source_node.category)
            || is_service_category(target_node.category);

        let mut touched = [source_component, target_component];
        if source_component == target_component {
            touched[1] = usize::MAX;
        }

        for component_index in touched.into_iter().filter(|index| *index != usize::MAX) {
            if mixed {
                components[component_index].mixed_dependency_count += 1;
            }
            if service_edge {
                components[component_index].service_coupling_count += 1;
            }

            for relation in &edge.evidence {
                let signature = EvidenceSignature {
                    relation: relation.relation,
                    source: relation.source.clone(),
                    target: relation.target.clone(),
                    anchors: relation.anchors.iter().cloned().collect(),
                };
                if evidence_seen[component_index].insert(signature) {
                    components[component_index].evidence_refs.push(EvidenceRef {
                        relation: relation.relation,
                        source: relation.source.clone(),
                        target: relation.target.clone(),
                        anchors: relation.anchors.clone(),
                    });
                }
                for anchor in &relation.anchors {
                    impacted_file_sets[component_index].insert(anchor.file_path.clone());
                }
            }
        }
    }

    for (index, component) in components.iter_mut().enumerate() {
        let mut impacted_files: Vec<_> = impacted_file_sets[index].drain().collect();
        impacted_files.sort();
        component.impacted_files = impacted_files;
        component.evidence_refs.sort_by(compare_evidence_ref);
    }
}

fn populate_component_diff_signals(components: &mut [ComponentData], diff: &GraphDiff) {
    let mut symbol_to_component: FxHashMap<SymbolSignature, usize> = FxHashMap::default();
    for (component_index, component) in components.iter().enumerate() {
        for signature in &component.symbol_signatures {
            symbol_to_component.insert(signature.clone(), component_index);
        }
    }

    let mut unresolved_canonical_ids: FxHashSet<String> = FxHashSet::default();
    let mut unresolved_symbols: FxHashSet<SymbolSignature> = FxHashSet::default();
    let mut residual_counts: FxHashMap<SymbolSignature, usize> = FxHashMap::default();

    let mut replacement_sets: Vec<FxHashSet<ReplacementSignature>> = (0..components.len())
        .map(|_| FxHashSet::default())
        .collect();
    let mut impacted_file_sets: Vec<FxHashSet<Utf8PathBuf>> = components
        .iter()
        .map(|component| component.impacted_files.iter().cloned().collect())
        .collect();

    for mapping in &diff.mappings {
        let legacy_signature = SymbolSignature::from_model_reference(&mapping.legacy_symbol);
        if mapping.status != MappingStatus::Matched {
            unresolved_canonical_ids.insert(mapping.legacy_canonical_id.clone());
            unresolved_symbols.insert(legacy_signature.clone());
        }

        if !mapping.status.has_target() {
            continue;
        }
        let Some(modern_symbol) = mapping.modern_symbol.as_ref() else {
            continue;
        };
        let Some(&component_index) = symbol_to_component.get(&legacy_signature) else {
            continue;
        };

        let modern_signature = SymbolSignature::from_model_reference(modern_symbol);
        let replacement_signature = ReplacementSignature {
            legacy: legacy_signature.clone(),
            modern: modern_signature,
        };
        if replacement_sets[component_index].insert(replacement_signature) {
            components[component_index]
                .suggested_replacements
                .push(SuggestedReplacement {
                    legacy_symbol: mapping.legacy_symbol.clone(),
                    modern_symbol: modern_symbol.clone(),
                    confidence_bps: mapping.confidence_bps,
                });
        }
    }

    for residual in &diff.residual_legacy_usages {
        let signature = SymbolSignature::from_model_reference(&residual.legacy_symbol);
        let count = residual_counts.entry(signature.clone()).or_insert(0);
        *count += 1;

        let Some(&component_index) = symbol_to_component.get(&signature) else {
            continue;
        };
        impacted_file_sets[component_index].insert(residual.file_path.clone());

        let modern_signature =
            SymbolSignature::from_model_reference(&residual.suggested_modern_symbol);
        let replacement_signature = ReplacementSignature {
            legacy: signature,
            modern: modern_signature,
        };
        if replacement_sets[component_index].insert(replacement_signature) {
            components[component_index]
                .suggested_replacements
                .push(SuggestedReplacement {
                    legacy_symbol: residual.legacy_symbol.clone(),
                    modern_symbol: residual.suggested_modern_symbol.clone(),
                    confidence_bps: residual.confidence_bps,
                });
        }
    }

    for (component_index, component) in components.iter_mut().enumerate() {
        let mut unresolved = component
            .legacy_canonical_ids
            .iter()
            .filter(|canonical_id| unresolved_canonical_ids.contains(*canonical_id))
            .count();
        for signature in &component.symbol_signatures {
            if unresolved_symbols.contains(signature) {
                unresolved += 1;
            }
            unresolved += residual_counts.get(signature).copied().unwrap_or(0);
        }
        component.unresolved_mapping_count = unresolved;

        let mut impacted_files: Vec<_> = impacted_file_sets[component_index].drain().collect();
        impacted_files.sort();
        component.impacted_files = impacted_files;

        component
            .suggested_replacements
            .sort_by(compare_suggested_replacement);
        component.suggested_replacements.dedup();
    }
}

fn score_component_risk(components: &mut [ComponentData], weights: PlannerRiskWeights) {
    let maxima = components
        .iter()
        .fold(SignalMaxima::default(), |mut maxima, component| {
            maxima.downstream_fanout = maxima.downstream_fanout.max(component.downstream_fanout);
            maxima.scc_size = maxima.scc_size.max(component.scc_size);
            maxima.mixed_dependency_count = maxima
                .mixed_dependency_count
                .max(component.mixed_dependency_count);
            maxima.service_coupling_count = maxima
                .service_coupling_count
                .max(component.service_coupling_count);
            maxima.unresolved_mapping_count = maxima
                .unresolved_mapping_count
                .max(component.unresolved_mapping_count);
            maxima
        });

    for component in components {
        let downstream = component_score(
            RiskSignalKind::DownstreamFanout,
            component.downstream_fanout,
            maxima.downstream_fanout,
            weights.downstream_fanout_bps,
        );
        let scc_size = component_score(
            RiskSignalKind::SccSize,
            component.scc_size,
            maxima.scc_size,
            weights.scc_size_bps,
        );
        let mixed = component_score(
            RiskSignalKind::MixedDependencyCount,
            component.mixed_dependency_count,
            maxima.mixed_dependency_count,
            weights.mixed_dependency_bps,
        );
        let service = component_score(
            RiskSignalKind::ServiceSurfaceCoupling,
            component.service_coupling_count,
            maxima.service_coupling_count,
            weights.service_coupling_bps,
        );
        let unresolved = component_score(
            RiskSignalKind::UnresolvedMappingPenalty,
            component.unresolved_mapping_count,
            maxima.unresolved_mapping_count,
            weights.unresolved_mapping_bps,
        );

        let mut breakdown = RiskBreakdown::default();
        breakdown
            .components
            .extend([downstream, scc_size, mixed, service, unresolved]);

        let total = breakdown.components.iter().fold(0_u32, |acc, score| {
            acc.saturating_add(u32::from(score.weighted_bps))
        });
        let bounded = total.min(u32::from(MAX_RISK_BPS));
        component.risk_score_bps = u16::try_from(bounded).ok().unwrap_or(MAX_RISK_BPS);
        component.risk_breakdown = breakdown;
    }
}

fn component_score(
    kind: RiskSignalKind,
    raw_value: usize,
    max_value: usize,
    weight_bps: u16,
) -> RiskComponentScore {
    let normalized_bps = normalize_signal(raw_value, max_value);
    let weighted = (u32::from(weight_bps) * u32::from(normalized_bps)) / u32::from(MAX_RISK_BPS);
    RiskComponentScore {
        kind,
        raw_value,
        normalized_bps,
        weighted_bps: u16::try_from(weighted).ok().unwrap_or(weight_bps),
    }
}

fn normalize_signal(raw_value: usize, max_value: usize) -> u16 {
    if raw_value == 0 || max_value == 0 {
        return 0;
    }

    let ratio = (raw_value.saturating_mul(usize::from(MAX_RISK_BPS)) / max_value)
        .min(usize::from(MAX_RISK_BPS));
    u16::try_from(ratio).ok().unwrap_or(MAX_RISK_BPS)
}

fn deterministic_topo_order(components: &[ComponentData]) -> Vec<usize> {
    let mut indegree: Vec<usize> = components
        .iter()
        .map(|component| component.predecessors.len())
        .collect();
    let mut frontier: Vec<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect();
    let mut order = Vec::with_capacity(components.len());

    while !frontier.is_empty() {
        frontier.sort_by(|left, right| compare_component_priority(components, *left, *right));
        let current = frontier.remove(0);
        order.push(current);

        for successor in &components[current].successors {
            if let Some(next) = indegree.get_mut(*successor) {
                *next = next.saturating_sub(1);
                if *next == 0 {
                    frontier.push(*successor);
                }
            }
        }
    }

    if order.len() != components.len() {
        let mut remaining: Vec<usize> = (0..components.len())
            .filter(|index| !order.contains(index))
            .collect();
        remaining.sort_by(|left, right| compare_component_priority(components, *left, *right));
        order.extend(remaining);
    }

    order
}

fn compare_component_priority(components: &[ComponentData], left: usize, right: usize) -> Ordering {
    components[left]
        .risk_score_bps
        .cmp(&components[right].risk_score_bps)
        .then_with(|| components[left].scc_size.cmp(&components[right].scc_size))
        .then_with(|| {
            components[left]
                .component_key
                .cmp(&components[right].component_key)
        })
}

fn build_plan_steps(
    components: &[ComponentData],
    ordered_components: &[usize],
    max_steps: Option<usize>,
) -> MigrationPlan {
    let total_components = ordered_components.len();
    let limit = max_steps.map_or(total_components, |max_steps| {
        max_steps.min(total_components)
    });
    let truncated_components = total_components.saturating_sub(limit);
    let selected = &ordered_components[..limit];

    let mut step_ids: FxHashMap<usize, String> = FxHashMap::default();
    for (step_index, component_index) in selected.iter().enumerate() {
        step_ids.insert(
            *component_index,
            format!("step-{index:04}", index = step_index + 1),
        );
    }

    let mut steps = Vec::with_capacity(limit);
    for (step_index, component_index) in selected.iter().enumerate() {
        let component = &components[*component_index];
        let Some(step_id) = step_ids.get(component_index).cloned() else {
            continue;
        };

        let mut prerequisites: Vec<String> = component
            .predecessors
            .iter()
            .filter_map(|predecessor| step_ids.get(predecessor).cloned())
            .collect();
        prerequisites.sort();

        let mut impacted_files = component.impacted_files.clone();
        impacted_files.sort();
        impacted_files.dedup();

        let mut evidence_refs = component.evidence_refs.clone();
        evidence_refs.sort_by(compare_evidence_ref);
        evidence_refs.dedup();

        let mut suggested_replacements = component.suggested_replacements.clone();
        suggested_replacements.sort_by(compare_suggested_replacement);
        suggested_replacements.dedup();

        steps.push(MigrationStep {
            step_id,
            order: step_index + 1,
            component_id: component.component_id.clone(),
            node_ids: component.node_ids.clone(),
            prerequisites,
            impacted_files,
            suggested_replacements,
            evidence_refs,
            risk_score_bps: component.risk_score_bps,
            risk_breakdown: component.risk_breakdown.clone(),
        });
    }

    MigrationPlan::new(steps, total_components, truncated_components)
}

fn compare_suggested_replacement(
    left: &SuggestedReplacement,
    right: &SuggestedReplacement,
) -> Ordering {
    compare_model_reference(&left.legacy_symbol, &right.legacy_symbol)
        .then_with(|| compare_model_reference(&left.modern_symbol, &right.modern_symbol))
        .then_with(|| right.confidence_bps.cmp(&left.confidence_bps))
}

fn compare_evidence_ref(left: &EvidenceRef, right: &EvidenceRef) -> Ordering {
    edge_kind_order(left.relation)
        .cmp(&edge_kind_order(right.relation))
        .then_with(|| compare_model_reference(&left.source, &right.source))
        .then_with(|| compare_model_reference(&left.target, &right.target))
        .then_with(|| left.anchors.len().cmp(&right.anchors.len()))
}

fn compare_model_reference(left: &ModelReference, right: &ModelReference) -> Ordering {
    source_order(left.source)
        .cmp(&source_order(right.source))
        .then_with(|| category_order(left.category).cmp(&category_order(right.category)))
        .then_with(|| left.name.cmp(&right.name))
}

fn is_service_edge_kind(kind: EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::ServiceParamType | EdgeKind::ServiceReturnType
    )
}

fn is_service_category(category: Option<ModelCategory>) -> bool {
    matches!(
        category,
        Some(ModelCategory::Service | ModelCategory::ServiceCodeGen)
    )
}

fn is_mixed_legacy_modern(source: SourceClassification, target: SourceClassification) -> bool {
    matches!(
        (source, target),
        (SourceClassification::Legacy, SourceClassification::Modern)
            | (SourceClassification::Modern, SourceClassification::Legacy)
    )
}

fn symbol_stem(symbol_name: &str) -> &str {
    for suffix in [
        "ServiceCodeGen",
        "CodeGenFormArray",
        "CodeGenForApi",
        "CodeGenForm",
        "CodeGen",
        "Service",
        "Model",
    ] {
        if let Some(base) = symbol_name.strip_suffix(suffix) {
            if !base.is_empty() {
                return base;
            }
        }
    }

    symbol_name
}

const fn source_order(source: ModelSource) -> u8 {
    match source {
        ModelSource::SharedLegacy => 0,
        ModelSource::Shared2023 => 1,
        _ => u8::MAX,
    }
}

const fn category_order(category: ModelCategory) -> u8 {
    match category {
        ModelCategory::Interface => 0,
        ModelCategory::Model => 1,
        ModelCategory::Service => 2,
        ModelCategory::CodeGen => 3,
        ModelCategory::CodeGenForApi => 4,
        ModelCategory::CodeGenForm => 5,
        ModelCategory::CodeGenFormArray => 6,
        ModelCategory::ServiceCodeGen => 7,
        _ => u8::MAX,
    }
}

const fn edge_kind_order(kind: EdgeKind) -> u8 {
    match kind {
        EdgeKind::ImportsSymbol => 0,
        EdgeKind::ReexportsSymbol => 1,
        EdgeKind::TypeRef => 2,
        EdgeKind::Extends => 3,
        EdgeKind::Implements => 4,
        EdgeKind::Constructs => 5,
        EdgeKind::FactoryCall => 6,
        EdgeKind::ServiceParamType => 7,
        EdgeKind::ServiceReturnType => 8,
        EdgeKind::ModelMapRegistration => 9,
        EdgeKind::LegacyBridge => 10,
        EdgeKind::GeneratedFrom => 11,
        _ => u8::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::DependencyGraphBuilder;
    use crate::inventory::build_inventory;
    use crate::mapping::{GraphDiff, ModelMapping};
    use ch_core::{
        FileId, FileInfo, MigrationStatus, ModelDefinition, ModelRegistry, SourceLocation,
    };
    use smallvec::{smallvec, SmallVec};

    fn definition(
        name: &str,
        source: ModelSource,
        definition_path: &str,
        exports: &[&str],
    ) -> ModelDefinition {
        let mut definition = ModelDefinition::new(name, source, definition_path);
        for export in exports {
            definition.add_export(*export);
        }
        definition
    }

    fn planner_inventory() -> crate::inventory::ModelInventory {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "Order",
            ModelSource::SharedLegacy,
            "shared/models/order.ts",
            &["OrderModel"],
        ));
        registry.register(definition(
            "Customer",
            ModelSource::SharedLegacy,
            "shared/models/customer.ts",
            &["CustomerModel"],
        ));
        registry.register(definition(
            "Order",
            ModelSource::Shared2023,
            "shared_2023/models/order.ts",
            &["OrderModel"],
        ));
        registry.register(definition(
            "Customer",
            ModelSource::Shared2023,
            "shared_2023/models/customer.ts",
            &["CustomerModel"],
        ));
        build_inventory(&registry)
    }

    fn cyclical_files() -> Vec<FileInfo> {
        let legacy_order = ModelReference::new(
            "OrderModel",
            ModelCategory::Interface,
            ModelSource::SharedLegacy,
        );
        let legacy_customer = ModelReference::new(
            "CustomerModel",
            ModelCategory::Interface,
            ModelSource::SharedLegacy,
        );
        let modern_order = ModelReference::new(
            "OrderModel",
            ModelCategory::Interface,
            ModelSource::Shared2023,
        );
        let modern_customer = ModelReference::new(
            "CustomerModel",
            ModelCategory::Interface,
            ModelSource::Shared2023,
        );

        let relation_a = relation(
            "src/cycle.ts",
            EdgeKind::Constructs,
            legacy_order.clone(),
            legacy_customer.clone(),
            11,
        );
        let relation_b = relation(
            "src/cycle.ts",
            EdgeKind::Constructs,
            legacy_customer.clone(),
            legacy_order.clone(),
            12,
        );
        let relation_c = relation(
            "src/bridge.ts",
            EdgeKind::LegacyBridge,
            legacy_customer,
            modern_order.clone(),
            13,
        );
        let relation_d = relation(
            "src/bridge.ts",
            EdgeKind::Constructs,
            modern_order.clone(),
            modern_customer.clone(),
            14,
        );

        let mut first = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/cycle.ts"));
        first.status = MigrationStatus::Partial;
        first.model_refs = smallvec![
            legacy_order.clone(),
            modern_order.clone(),
            modern_customer.clone()
        ];
        first.relation_evidence = smallvec![relation_a, relation_c];

        let mut second = FileInfo::new(FileId::new(2), Utf8PathBuf::from("src/bridge.ts"));
        second.status = MigrationStatus::Legacy;
        second.model_refs = smallvec![legacy_order, modern_order, modern_customer];
        second.relation_evidence = smallvec![relation_b, relation_d];

        vec![first, second]
    }

    fn relation(
        file_path: &str,
        kind: EdgeKind,
        source: ModelReference,
        target: ModelReference,
        snippet_hash: u64,
    ) -> ch_core::AstRelationEvidence {
        let mut relation = ch_core::AstRelationEvidence::new(kind, source, target);
        let mut anchor = CstAnchor::new(
            file_path,
            10,
            24,
            SourceLocation::new(1, 0, 10),
            SourceLocation::new(1, 14, 24),
            "identifier",
        );
        anchor.snippet_hash = snippet_hash;
        relation.add_anchor(anchor);
        relation
    }

    fn mapping(
        legacy_canonical_id: &str,
        legacy_symbol: ModelReference,
        modern_symbol: Option<ModelReference>,
        status: MappingStatus,
        confidence_bps: u16,
    ) -> ModelMapping {
        ModelMapping {
            legacy_canonical_id: legacy_canonical_id.to_owned(),
            legacy_symbol,
            modern_canonical_id: modern_symbol
                .as_ref()
                .map(|symbol| pascal_to_kebab(symbol_stem(symbol.name.as_str()))),
            modern_symbol,
            confidence_bps,
            status,
            reasons: SmallVec::new(),
        }
    }

    #[test]
    fn test_plan_orders_components_and_has_valid_prerequisites() {
        let inventory = planner_inventory();
        let files = cyclical_files();
        let graph = DependencyGraphBuilder::new().build(&inventory, &files);
        let plan = GraphPlanner::new().plan(&graph, &GraphDiff::default());

        assert_eq!(plan.counts.total_components, plan.steps.len());
        assert!(!plan.is_empty());
        assert!(plan.steps.iter().any(|step| step.node_ids.len() >= 2));
        assert!(plan
            .steps
            .iter()
            .flat_map(|step| step.impacted_files.iter())
            .any(|path| path.as_str() == "src/cycle.ts"));

        for step in &plan.steps {
            for prerequisite in &step.prerequisites {
                let prerequisite_order = plan
                    .steps
                    .iter()
                    .find(|candidate| &candidate.step_id == prerequisite)
                    .map(|candidate| candidate.order);
                assert!(prerequisite_order.is_some());
                if let Some(prerequisite_order) = prerequisite_order {
                    assert!(prerequisite_order < step.order);
                }
            }
        }
    }

    #[test]
    fn test_plan_is_deterministic_across_file_order() {
        let inventory = planner_inventory();
        let files = cyclical_files();
        let graph_a = DependencyGraphBuilder::new().build(&inventory, &files);

        let mut reversed_files = files.clone();
        reversed_files.reverse();
        let graph_b = DependencyGraphBuilder::new().build(&inventory, &reversed_files);

        let planner = GraphPlanner::new();
        let plan_a = planner.plan(&graph_a, &GraphDiff::default());
        let plan_b = planner.plan(&graph_b, &GraphDiff::default());
        assert_eq!(plan_a, plan_b);
    }

    #[test]
    fn test_plan_applies_max_steps_and_filters_prerequisites() {
        let inventory = planner_inventory();
        let files = cyclical_files();
        let graph = DependencyGraphBuilder::new().build(&inventory, &files);

        let planner = GraphPlanner::with_config(PlannerConfig {
            max_steps: Some(2),
            ..PlannerConfig::default()
        });
        let plan = planner.plan(&graph, &GraphDiff::default());

        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.counts.total_components, 3);
        assert_eq!(plan.counts.truncated_components, 1);

        let step_ids: FxHashSet<String> =
            plan.steps.iter().map(|step| step.step_id.clone()).collect();
        for step in &plan.steps {
            for prerequisite in &step.prerequisites {
                assert!(step_ids.contains(prerequisite));
            }
        }
    }

    #[test]
    fn test_risk_breakdown_is_explainable() {
        let inventory = planner_inventory();
        let files = cyclical_files();
        let graph = DependencyGraphBuilder::new().build(&inventory, &files);
        let plan = GraphPlanner::new().plan(&graph, &GraphDiff::default());

        for step in &plan.steps {
            assert_eq!(step.risk_breakdown.components.len(), 5);
            let total = step
                .risk_breakdown
                .components
                .iter()
                .fold(0_u32, |acc, component| {
                    acc.saturating_add(u32::from(component.weighted_bps))
                });
            let bounded_total = u16::try_from(total.min(u32::from(MAX_RISK_BPS)))
                .ok()
                .unwrap_or(MAX_RISK_BPS);
            assert_eq!(bounded_total, step.risk_score_bps);
        }
    }

    #[test]
    fn test_unresolved_mapping_penalty_affects_ordering() {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "Foo",
            ModelSource::SharedLegacy,
            "shared/models/foo.ts",
            &["FooModel"],
        ));
        registry.register(definition(
            "Bar",
            ModelSource::SharedLegacy,
            "shared/models/bar.ts",
            &["BarModel"],
        ));
        let inventory = build_inventory(&registry);

        let foo = ModelReference::new(
            "FooModel",
            ModelCategory::Interface,
            ModelSource::SharedLegacy,
        );
        let bar = ModelReference::new(
            "BarModel",
            ModelCategory::Interface,
            ModelSource::SharedLegacy,
        );
        let modern_bar = ModelReference::new(
            "BarModel",
            ModelCategory::Interface,
            ModelSource::Shared2023,
        );

        let mut file = FileInfo::new(FileId::new(3), Utf8PathBuf::from("src/isolated.ts"));
        file.status = MigrationStatus::Legacy;
        file.model_refs = smallvec![foo.clone(), bar.clone()];

        let graph = DependencyGraphBuilder::new().build(&inventory, &[file]);
        let diff = GraphDiff::new(
            vec![
                mapping("foo", foo.clone(), None, MappingStatus::NoMatch, 0),
                mapping(
                    "bar",
                    bar.clone(),
                    Some(modern_bar),
                    MappingStatus::Matched,
                    MAX_CONFIDENCE_BPS,
                ),
            ],
            Vec::new(),
        );

        let plan = GraphPlanner::new().plan(&graph, &diff);
        assert_eq!(plan.steps.len(), 2);

        let foo_position = plan.steps.iter().position(|step| {
            step.node_ids
                .iter()
                .any(|node_id| node_id.contains("FooModel"))
        });
        let bar_position = plan.steps.iter().position(|step| {
            step.node_ids
                .iter()
                .any(|node_id| node_id.contains("BarModel"))
        });

        assert!(foo_position.is_some());
        assert!(bar_position.is_some());
        if let (Some(foo_position), Some(bar_position)) = (foo_position, bar_position) {
            assert!(bar_position < foo_position);
        }
    }
}
