//! Deterministic dependency graph builder.
//!
//! This module builds a typed [`crate::graph::DependencyGraph`] from
//! inventory and scanner outputs while preserving deterministic node/edge
//! ordering and edge-level relation evidence.

use std::cmp::Ordering;

use camino::Utf8PathBuf;
use ch_core::{
    AstRelationEvidence, CstAnchor, EdgeKind, FileInfo, FxHashMap, FxHashSet, ModelArtifact,
    ModelCategory, ModelReference, ModelSource,
};
use petgraph::graph::NodeIndex;
use smallvec::SmallVec;

use crate::graph::{
    DependencyGraph, DependencyStableGraph, GraphCounts, GraphEdge, GraphEdgeKindCount,
    GraphMetadata, GraphNode, GraphNodeKind, ParserMetadata,
};
use crate::inventory::{ModelInventory, ModelInventoryRecord};

/// Builder for deterministic dependency graphs.
#[derive(Debug, Clone, Default)]
pub struct DependencyGraphBuilder {
    source_roots: Vec<Utf8PathBuf>,
    parser_metadata: ParserMetadata,
}

impl DependencyGraphBuilder {
    /// Creates a new graph builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets source roots used for graph metadata.
    #[must_use]
    pub fn with_source_roots(mut self, mut source_roots: Vec<Utf8PathBuf>) -> Self {
        source_roots.sort();
        source_roots.dedup();
        self.source_roots = source_roots;
        self
    }

    /// Sets parser metadata placeholders for graph metadata.
    #[must_use]
    pub fn with_parser_metadata(
        mut self,
        parser_version: Option<String>,
        relation_query_version: Option<String>,
    ) -> Self {
        self.parser_metadata = ParserMetadata { parser_version, relation_query_version };
        self
    }

    /// Builds a deterministic dependency graph from inventory and scanner files.
    #[must_use]
    pub fn build(&self, inventory: &ModelInventory, files: &[FileInfo]) -> DependencyGraph {
        let mut graph = DependencyStableGraph::default();
        let mut node_indices: FxHashMap<String, NodeIndex<u32>> = FxHashMap::default();

        for node in inventory_nodes(inventory) {
            register_node(&mut graph, &mut node_indices, node);
        }

        let mut sorted_files: Vec<&FileInfo> = files.iter().collect();
        sorted_files.sort_by(|left, right| {
            left.path.as_str().cmp(right.path.as_str()).then_with(|| left.id.0.cmp(&right.id.0))
        });

        for file in &sorted_files {
            let node = GraphNode::file(file_node_id(file.path.as_str()), file.path.clone());
            register_node(&mut graph, &mut node_indices, node);
        }

        for symbol in collect_symbol_keys(&sorted_files) {
            let node = GraphNode::symbol(
                symbol_node_id(symbol.source, symbol.category, symbol.name.as_str()),
                symbol.source,
                symbol.name,
                symbol.category,
            );
            register_node(&mut graph, &mut node_indices, node);
        }

        for edge in aggregate_relation_edges(&sorted_files) {
            let Some(source) = node_indices.get(edge.source_node_id.as_str()).copied() else {
                continue;
            };
            let Some(target) = node_indices.get(edge.target_node_id.as_str()).copied() else {
                continue;
            };
            graph.add_edge(source, target, GraphEdge::new(edge.kind, edge.evidence));
        }

        let metadata = GraphMetadata {
            counts: build_counts(&graph),
            source_roots: self.source_roots.clone(),
            parser_metadata: self.parser_metadata.clone(),
        };

        DependencyGraph::new(graph, metadata)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SymbolKey {
    source: ModelSource,
    category: ModelCategory,
    name: String,
}

impl SymbolKey {
    fn from_model_reference(model_ref: &ModelReference) -> Self {
        Self {
            source: model_ref.source,
            category: model_ref.category,
            name: model_ref.name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EdgeKey {
    source_node_id: String,
    target_node_id: String,
    kind: EdgeKind,
}

#[derive(Debug, Default)]
struct EdgeAccumulator {
    evidence: SmallVec<[AstRelationEvidence; 2]>,
    seen: FxHashSet<EvidenceSignature>,
}

#[derive(Debug, Clone)]
struct AggregatedEdge {
    source_node_id: String,
    target_node_id: String,
    kind: EdgeKind,
    evidence: SmallVec<[AstRelationEvidence; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EvidenceSignature {
    relation: EdgeKind,
    source: ModelReference,
    target: ModelReference,
    anchors: Vec<CstAnchor>,
}

fn inventory_nodes(inventory: &ModelInventory) -> Vec<GraphNode> {
    let mut nodes = Vec::new();

    for record in inventory.records() {
        push_inventory_node(
            &mut nodes,
            record,
            record.interface.as_ref(),
            GraphNodeKind::Interface,
        );
        push_inventory_node(
            &mut nodes,
            record,
            record.codegen_interface.as_ref(),
            GraphNodeKind::Interface,
        );
        push_inventory_node(&mut nodes, record, record.wrapper.as_ref(), GraphNodeKind::Model);
        push_inventory_node(&mut nodes, record, record.codegen.as_ref(), GraphNodeKind::Model);
        push_inventory_node(
            &mut nodes,
            record,
            record.service_wrapper.as_ref(),
            GraphNodeKind::Service,
        );
        push_inventory_node(
            &mut nodes,
            record,
            record.service_codegen.as_ref(),
            GraphNodeKind::Service,
        );
    }

    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    nodes
}

fn push_inventory_node(
    nodes: &mut Vec<GraphNode>,
    record: &ModelInventoryRecord,
    artifact: Option<&ModelArtifact>,
    kind: GraphNodeKind,
) {
    let Some(artifact) = artifact else {
        return;
    };

    let node = match kind {
        GraphNodeKind::Interface => GraphNode::interface(
            interface_node_id(record.source, &record.canonical_id, &artifact.export_name),
            record.source,
            record.canonical_id.clone(),
            artifact.model_name.clone(),
            artifact.category,
            artifact.export_name.clone(),
            artifact.definition_path.clone(),
        ),
        GraphNodeKind::Model => GraphNode::model(
            model_node_id(record.source, &record.canonical_id, &artifact.export_name),
            record.source,
            record.canonical_id.clone(),
            artifact.model_name.clone(),
            artifact.category,
            artifact.export_name.clone(),
            artifact.definition_path.clone(),
        ),
        GraphNodeKind::Service => GraphNode::service(
            service_node_id(record.source, &record.canonical_id, &artifact.export_name),
            record.source,
            record.canonical_id.clone(),
            artifact.model_name.clone(),
            artifact.category,
            artifact.export_name.clone(),
            artifact.definition_path.clone(),
        ),
        _ => return,
    };

    nodes.push(node);
}

fn collect_symbol_keys(files: &[&FileInfo]) -> Vec<SymbolKey> {
    let mut keys = Vec::new();

    for file in files {
        for model_ref in &file.model_refs {
            keys.push(SymbolKey::from_model_reference(model_ref));
        }

        for relation in &file.relation_evidence {
            keys.push(SymbolKey::from_model_reference(&relation.source));
            keys.push(SymbolKey::from_model_reference(&relation.target));
        }
    }

    keys.sort_by(|left, right| {
        source_order(left.source)
            .cmp(&source_order(right.source))
            .then_with(|| category_order(left.category).cmp(&category_order(right.category)))
            .then_with(|| left.name.cmp(&right.name))
    });
    keys.dedup_by(|left, right| {
        left.source == right.source && left.category == right.category && left.name == right.name
    });
    keys
}

fn aggregate_relation_edges(files: &[&FileInfo]) -> Vec<AggregatedEdge> {
    let mut edge_map: FxHashMap<EdgeKey, EdgeAccumulator> = FxHashMap::default();

    for file in files {
        let mut relations: Vec<&AstRelationEvidence> = file.relation_evidence.iter().collect();
        relations.sort_by(|left, right| compare_relation_evidence(left, right));

        for relation in relations {
            let edge_key = EdgeKey {
                source_node_id: symbol_node_id(
                    relation.source.source,
                    relation.source.category,
                    relation.source.name.as_str(),
                ),
                target_node_id: symbol_node_id(
                    relation.target.source,
                    relation.target.category,
                    relation.target.name.as_str(),
                ),
                kind: relation.relation,
            };

            let signature = evidence_signature(relation);
            let accumulator = edge_map.entry(edge_key).or_default();
            if accumulator.seen.insert(signature) {
                accumulator.evidence.push(relation.clone());
            }
        }
    }

    let mut aggregated: Vec<_> = edge_map
        .into_iter()
        .filter_map(|(key, accumulator)| {
            if accumulator.evidence.is_empty() {
                return None;
            }
            Some(AggregatedEdge {
                source_node_id: key.source_node_id,
                target_node_id: key.target_node_id,
                kind: key.kind,
                evidence: accumulator.evidence,
            })
        })
        .collect();

    aggregated.sort_by(|left, right| {
        left.source_node_id
            .cmp(&right.source_node_id)
            .then_with(|| left.target_node_id.cmp(&right.target_node_id))
            .then_with(|| edge_kind_order(left.kind).cmp(&edge_kind_order(right.kind)))
    });
    aggregated
}

fn compare_relation_evidence(left: &AstRelationEvidence, right: &AstRelationEvidence) -> Ordering {
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

fn evidence_signature(relation: &AstRelationEvidence) -> EvidenceSignature {
    EvidenceSignature {
        relation: relation.relation,
        source: relation.source.clone(),
        target: relation.target.clone(),
        anchors: relation.anchors.iter().cloned().collect(),
    }
}

fn register_node(
    graph: &mut DependencyStableGraph,
    node_indices: &mut FxHashMap<String, NodeIndex<u32>>,
    node: GraphNode,
) -> NodeIndex<u32> {
    if let Some(existing) = node_indices.get(node.node_id.as_str()).copied() {
        return existing;
    }

    let node_id = node.node_id.clone();
    let index = graph.add_node(node);
    node_indices.insert(node_id, index);
    index
}

fn build_counts(graph: &DependencyStableGraph) -> GraphCounts {
    let mut counts = GraphCounts {
        total_nodes: graph.node_count(),
        total_edges: graph.edge_count(),
        ..GraphCounts::default()
    };

    for node in graph.node_weights() {
        match node.kind {
            GraphNodeKind::Interface => counts.interface_nodes += 1,
            GraphNodeKind::Model => counts.model_nodes += 1,
            GraphNodeKind::Service => counts.service_nodes += 1,
            GraphNodeKind::File => counts.file_nodes += 1,
            GraphNodeKind::Symbol => counts.symbol_nodes += 1,
        }
    }

    let mut edges_by_kind: FxHashMap<EdgeKind, usize> = FxHashMap::default();
    for edge in graph.edge_weights() {
        let entry = edges_by_kind.entry(edge.kind).or_insert(0);
        *entry += 1;
    }

    let mut edge_kind_counts: Vec<_> =
        edges_by_kind.into_iter().map(|(kind, count)| GraphEdgeKindCount { kind, count }).collect();
    edge_kind_counts
        .sort_by(|left, right| edge_kind_order(left.kind).cmp(&edge_kind_order(right.kind)));
    counts.edges_by_kind = edge_kind_counts;
    counts
}

fn file_node_id(path: &str) -> String {
    format!("file:{path}")
}

fn symbol_node_id(source: ModelSource, category: ModelCategory, symbol_name: &str) -> String {
    format!("symbol:{}:{}:{symbol_name}", source_key(source), category_key(category))
}

fn interface_node_id(source: ModelSource, canonical_id: &str, export_name: &str) -> String {
    format!("interface:{}:{canonical_id}:{export_name}", source_key(source))
}

fn model_node_id(source: ModelSource, canonical_id: &str, export_name: &str) -> String {
    format!("model:{}:{canonical_id}:{export_name}", source_key(source))
}

fn service_node_id(source: ModelSource, canonical_id: &str, export_name: &str) -> String {
    format!("service:{}:{canonical_id}:{export_name}", source_key(source))
}

fn source_key(source: ModelSource) -> &'static str {
    match source {
        ModelSource::SharedLegacy => "legacy",
        ModelSource::Shared2023 => "modern",
        _ => "unknown",
    }
}

fn source_order(source: ModelSource) -> u8 {
    match source {
        ModelSource::SharedLegacy => 0,
        ModelSource::Shared2023 => 1,
        _ => u8::MAX,
    }
}

fn category_key(category: ModelCategory) -> &'static str {
    match category {
        ModelCategory::Interface => "interface",
        ModelCategory::Model => "model",
        ModelCategory::Service => "service",
        ModelCategory::CodeGen => "codegen",
        ModelCategory::CodeGenForApi => "codegen_for_api",
        ModelCategory::CodeGenForm => "codegen_form",
        ModelCategory::CodeGenFormArray => "codegen_form_array",
        ModelCategory::ServiceCodeGen => "service_codegen",
        _ => "unknown",
    }
}

fn category_order(category: ModelCategory) -> u8 {
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

fn edge_kind_order(kind: EdgeKind) -> u8 {
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
    use crate::inventory::build_inventory;
    use ch_core::{FileId, MigrationStatus, ModelDefinition, ModelRegistry, SourceLocation};
    use petgraph::visit::{EdgeRef, IntoEdgeReferences};
    use smallvec::smallvec;

    #[test]
    fn test_build_produces_typed_graph_with_evidence() {
        let inventory = sample_inventory();
        let files = sample_files();

        let graph = DependencyGraphBuilder::new()
            .with_source_roots(vec![
                Utf8PathBuf::from("src"),
                Utf8PathBuf::from("shared"),
                Utf8PathBuf::from("src"),
            ])
            .with_parser_metadata(
                Some("tree-sitter-0.24".to_owned()),
                Some("relations-v1".to_owned()),
            )
            .build(&inventory, &files);

        assert!(graph.node_count() > 0);
        assert!(graph.edge_count() > 0);
        assert_eq!(graph.metadata().counts.total_nodes, graph.node_count());
        assert_eq!(graph.metadata().counts.total_edges, graph.edge_count());
        assert_eq!(
            graph.metadata().source_roots,
            vec![Utf8PathBuf::from("shared"), Utf8PathBuf::from("src")]
        );

        for edge in graph.graph().edge_weights() {
            assert!(edge.has_evidence());
        }

        let kinds: FxHashSet<EdgeKind> =
            graph.metadata().counts.edges_by_kind.iter().map(|entry| entry.kind).collect();
        assert!(kinds.contains(&EdgeKind::LegacyBridge));
        assert!(kinds.contains(&EdgeKind::Constructs));
    }

    #[test]
    fn test_build_is_deterministic_across_input_ordering() {
        let inventory = sample_inventory();
        let files = sample_files();

        let builder = DependencyGraphBuilder::new();
        let graph_a = builder.build(&inventory, &files);

        let mut reversed_files = files.clone();
        reversed_files.reverse();
        let graph_b = builder.build(&inventory, &reversed_files);

        assert_eq!(collect_node_ids(&graph_a), collect_node_ids(&graph_b));
        assert_eq!(collect_edge_fingerprints(&graph_a), collect_edge_fingerprints(&graph_b));
        assert_eq!(graph_a.metadata().counts, graph_b.metadata().counts);
    }

    #[test]
    fn test_relation_edge_evidence_is_deduplicated() {
        let inventory = sample_inventory();

        let source =
            ModelReference::new("OrderModel", ModelCategory::Interface, ModelSource::SharedLegacy);
        let target =
            ModelReference::new("OrderModel", ModelCategory::Interface, ModelSource::Shared2023);

        let relation = make_relation(
            "src/components/duplicate.ts",
            EdgeKind::LegacyBridge,
            source,
            target,
            42,
        );

        let mut file =
            FileInfo::new(FileId::new(10), Utf8PathBuf::from("src/components/duplicate.ts"));
        file.status = MigrationStatus::Partial;
        file.relation_evidence = smallvec![relation.clone(), relation];

        let graph = DependencyGraphBuilder::new().build(&inventory, &[file]);

        assert_eq!(graph.edge_count(), 1);
        let mut edge_weights = graph.graph().edge_weights();
        let edge = edge_weights.next();
        assert!(edge.is_some());
        let edge = edge.unwrap_or_else(|| unreachable!());
        assert_eq!(edge.evidence.len(), 1);
        assert!(edge.has_evidence());
    }

    fn sample_inventory() -> ModelInventory {
        let mut registry = ModelRegistry::new();

        registry.register(ModelDefinition {
            name: "Order".to_owned(),
            source: ModelSource::SharedLegacy,
            definition_path: Utf8PathBuf::from("shared/models/order.ts"),
            exports: smallvec![
                "OrderModel".to_owned(),
                "Order".to_owned(),
                "OrderCodeGen".to_owned(),
                "OrderService".to_owned(),
            ],
        });

        registry.register(ModelDefinition {
            name: "Order".to_owned(),
            source: ModelSource::Shared2023,
            definition_path: Utf8PathBuf::from("shared_2023/models/order.ts"),
            exports: smallvec![
                "OrderModel".to_owned(),
                "Order".to_owned(),
                "OrderCodeGen".to_owned(),
                "OrderService".to_owned(),
            ],
        });

        build_inventory(&registry)
    }

    fn sample_files() -> Vec<FileInfo> {
        let legacy_order =
            ModelReference::new("OrderModel", ModelCategory::Interface, ModelSource::SharedLegacy);
        let modern_order =
            ModelReference::new("OrderModel", ModelCategory::Interface, ModelSource::Shared2023);
        let modern_wrapper =
            ModelReference::new("Order", ModelCategory::Model, ModelSource::Shared2023);

        let relation_bridge = make_relation(
            "src/components/order.ts",
            EdgeKind::LegacyBridge,
            legacy_order.clone(),
            modern_order.clone(),
            11,
        );
        let relation_constructs = make_relation(
            "src/components/order.ts",
            EdgeKind::Constructs,
            modern_order.clone(),
            modern_wrapper.clone(),
            12,
        );

        let mut file_b = FileInfo::new(FileId::new(2), Utf8PathBuf::from("src/b.ts"));
        file_b.status = MigrationStatus::Partial;
        file_b.model_refs = smallvec![legacy_order.clone(), modern_order.clone(), modern_wrapper];
        file_b.relation_evidence = smallvec![relation_bridge, relation_constructs];

        let mut file_a = FileInfo::new(FileId::new(1), Utf8PathBuf::from("src/a.ts"));
        file_a.status = MigrationStatus::Migrated;
        file_a.model_refs = smallvec![modern_order];

        vec![file_b, file_a]
    }

    fn make_relation(
        file_path: &str,
        kind: EdgeKind,
        source: ModelReference,
        target: ModelReference,
        snippet_hash: u64,
    ) -> AstRelationEvidence {
        let mut relation = AstRelationEvidence::new(kind, source, target);
        let mut anchor = CstAnchor::new(
            file_path,
            10,
            22,
            SourceLocation::new(1, 0, 10),
            SourceLocation::new(1, 12, 22),
            "call_expression",
        );
        anchor.snippet_hash = snippet_hash;
        relation.add_anchor(anchor);
        relation
    }

    fn collect_node_ids(graph: &DependencyGraph) -> Vec<String> {
        graph
            .graph()
            .node_indices()
            .map(|node_index| graph.graph()[node_index].node_id.clone())
            .collect()
    }

    fn collect_edge_fingerprints(graph: &DependencyGraph) -> Vec<(String, String, u8, usize)> {
        let mut fingerprints: Vec<_> = graph
            .graph()
            .edge_references()
            .map(|edge| {
                let source = graph.graph()[edge.source()].node_id.clone();
                let target = graph.graph()[edge.target()].node_id.clone();
                let order = edge_kind_order(edge.weight().kind);
                let evidence_count = edge.weight().evidence.len();
                (source, target, order, evidence_count)
            })
            .collect();
        fingerprints.sort();
        fingerprints
    }

    #[test]
    fn test_symbol_key_collection_is_deterministic_and_unique() {
        let files = sample_files();
        let file_refs: Vec<&FileInfo> = files.iter().collect();
        let keys = collect_symbol_keys(&file_refs);

        let mut unique_signatures: FxHashSet<(ModelSource, ModelCategory, String)> =
            FxHashSet::default();
        for key in &keys {
            let inserted = unique_signatures.insert((key.source, key.category, key.name.clone()));
            assert!(inserted);
        }

        let mut previous: Option<&SymbolKey> = None;
        for key in &keys {
            if let Some(last) = previous {
                let ordered = source_order(last.source) < source_order(key.source)
                    || (source_order(last.source) == source_order(key.source)
                        && (category_order(last.category) < category_order(key.category)
                            || (category_order(last.category) == category_order(key.category)
                                && last.name <= key.name)));
                assert!(ordered);
            }
            previous = Some(key);
        }
    }
}
