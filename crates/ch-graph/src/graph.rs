//! Graph data contracts for model dependency analysis.
//!
//! This module defines the typed node/edge payloads and metadata returned by
//! the graph builder. The payloads are designed for deterministic downstream
//! comparison, planning, and artifact export.

use camino::Utf8PathBuf;
use ch_core::{AstRelationEvidence, EdgeKind, ModelCategory, ModelSource, SourceClassification};
use petgraph::stable_graph::StableDiGraph;

/// Directed stable dependency graph backing type.
pub type DependencyStableGraph = StableDiGraph<GraphNode, GraphEdge, u32>;

/// Logical node categories for migration dependency analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphNodeKind {
    /// Interface-level model artifact node.
    Interface,
    /// Model-level artifact node.
    Model,
    /// Service-level artifact node.
    Service,
    /// Consumer file node.
    File,
    /// Symbol node (imported/reference model symbol).
    Symbol,
}

/// A typed node in the dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    /// Stable deterministic identifier for this node.
    pub node_id: String,
    /// Node kind.
    pub kind: GraphNodeKind,
    /// Legacy/modern/unknown ecosystem classification.
    pub source_classification: SourceClassification,
    /// Source ecosystem when known.
    pub source: Option<ModelSource>,
    /// Canonical model identifier for inventory-derived nodes.
    pub canonical_id: Option<String>,
    /// Logical model name when known.
    pub model_name: Option<String>,
    /// Symbol name for symbol nodes.
    pub symbol_name: Option<String>,
    /// Artifact category when known.
    pub category: Option<ModelCategory>,
    /// Exported symbol name when relevant.
    pub export_name: Option<String>,
    /// Definition path for inventory-derived artifacts.
    pub definition_path: Option<Utf8PathBuf>,
    /// File path for consumer file nodes.
    pub file_path: Option<Utf8PathBuf>,
}

impl GraphNode {
    /// Constructs an interface node from inventory metadata.
    #[must_use]
    pub fn interface(
        node_id: String,
        source: ModelSource,
        canonical_id: String,
        model_name: String,
        category: ModelCategory,
        export_name: String,
        definition_path: Utf8PathBuf,
    ) -> Self {
        Self {
            node_id,
            kind: GraphNodeKind::Interface,
            source_classification: source_classification(source),
            source: Some(source),
            canonical_id: Some(canonical_id),
            model_name: Some(model_name),
            symbol_name: None,
            category: Some(category),
            export_name: Some(export_name),
            definition_path: Some(definition_path),
            file_path: None,
        }
    }

    /// Constructs a model node from inventory metadata.
    #[must_use]
    pub fn model(
        node_id: String,
        source: ModelSource,
        canonical_id: String,
        model_name: String,
        category: ModelCategory,
        export_name: String,
        definition_path: Utf8PathBuf,
    ) -> Self {
        Self {
            node_id,
            kind: GraphNodeKind::Model,
            source_classification: source_classification(source),
            source: Some(source),
            canonical_id: Some(canonical_id),
            model_name: Some(model_name),
            symbol_name: None,
            category: Some(category),
            export_name: Some(export_name),
            definition_path: Some(definition_path),
            file_path: None,
        }
    }

    /// Constructs a service node from inventory metadata.
    #[must_use]
    pub fn service(
        node_id: String,
        source: ModelSource,
        canonical_id: String,
        model_name: String,
        category: ModelCategory,
        export_name: String,
        definition_path: Utf8PathBuf,
    ) -> Self {
        Self {
            node_id,
            kind: GraphNodeKind::Service,
            source_classification: source_classification(source),
            source: Some(source),
            canonical_id: Some(canonical_id),
            model_name: Some(model_name),
            symbol_name: None,
            category: Some(category),
            export_name: Some(export_name),
            definition_path: Some(definition_path),
            file_path: None,
        }
    }

    /// Constructs a file node.
    #[must_use]
    pub fn file(node_id: String, file_path: Utf8PathBuf) -> Self {
        Self {
            node_id,
            kind: GraphNodeKind::File,
            source_classification: SourceClassification::Unknown,
            source: None,
            canonical_id: None,
            model_name: None,
            symbol_name: None,
            category: None,
            export_name: None,
            definition_path: None,
            file_path: Some(file_path),
        }
    }

    /// Constructs a symbol node.
    #[must_use]
    pub fn symbol(
        node_id: String,
        source: ModelSource,
        symbol_name: String,
        category: ModelCategory,
    ) -> Self {
        Self {
            node_id,
            kind: GraphNodeKind::Symbol,
            source_classification: source_classification(source),
            source: Some(source),
            canonical_id: None,
            model_name: None,
            symbol_name: Some(symbol_name),
            category: Some(category),
            export_name: None,
            definition_path: None,
            file_path: None,
        }
    }
}

/// A typed graph edge containing normalized relation evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    /// Edge semantics.
    pub kind: EdgeKind,
    /// Evidence entries supporting this edge.
    pub evidence: smallvec::SmallVec<[AstRelationEvidence; 2]>,
}

impl GraphEdge {
    /// Creates a graph edge payload.
    #[must_use]
    pub fn new(kind: EdgeKind, evidence: smallvec::SmallVec<[AstRelationEvidence; 2]>) -> Self {
        Self { kind, evidence }
    }

    /// Returns `true` when the edge has at least one evidence entry.
    #[must_use]
    pub fn has_evidence(&self) -> bool {
        !self.evidence.is_empty()
    }
}

/// Edge count grouped by relation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphEdgeKindCount {
    /// Relation kind.
    pub kind: EdgeKind,
    /// Number of edges with this relation kind.
    pub count: usize,
}

/// Deterministic graph counts for reporting and verification.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphCounts {
    /// Total node count.
    pub total_nodes: usize,
    /// Total edge count.
    pub total_edges: usize,
    /// Interface node count.
    pub interface_nodes: usize,
    /// Model node count.
    pub model_nodes: usize,
    /// Service node count.
    pub service_nodes: usize,
    /// File node count.
    pub file_nodes: usize,
    /// Symbol node count.
    pub symbol_nodes: usize,
    /// Edge counts grouped by relation kind.
    pub edges_by_kind: Vec<GraphEdgeKindCount>,
}

/// Parser metadata placeholders captured at graph build boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParserMetadata {
    /// Parser version identifier.
    pub parser_version: Option<String>,
    /// Relation query version identifier.
    pub relation_query_version: Option<String>,
}

/// Metadata for a built dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphMetadata {
    /// Deterministic graph counts.
    pub counts: GraphCounts,
    /// Source roots used to build the graph.
    pub source_roots: Vec<Utf8PathBuf>,
    /// Parser metadata placeholders.
    pub parser_metadata: ParserMetadata,
}

/// Complete dependency graph output with metadata.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    graph: DependencyStableGraph,
    metadata: GraphMetadata,
}

impl DependencyGraph {
    /// Creates a dependency graph from graph payload and metadata.
    #[must_use]
    pub fn new(graph: DependencyStableGraph, metadata: GraphMetadata) -> Self {
        Self { graph, metadata }
    }

    /// Returns the stable graph payload.
    #[must_use]
    pub fn graph(&self) -> &DependencyStableGraph {
        &self.graph
    }

    /// Returns graph metadata.
    #[must_use]
    pub fn metadata(&self) -> &GraphMetadata {
        &self.metadata
    }

    /// Returns the number of nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Returns the number of edges.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Consumes the graph into its parts.
    #[must_use]
    pub fn into_parts(self) -> (DependencyStableGraph, GraphMetadata) {
        (self.graph, self.metadata)
    }
}

#[inline]
fn source_classification(source: ModelSource) -> SourceClassification {
    match source {
        ModelSource::SharedLegacy => SourceClassification::Legacy,
        ModelSource::Shared2023 => SourceClassification::Modern,
        _ => SourceClassification::Unknown,
    }
}
