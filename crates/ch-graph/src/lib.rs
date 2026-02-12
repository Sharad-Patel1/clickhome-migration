//! Dependency graph foundations for migration planning.
//!
//! This crate introduces graph-focused primitives that downstream tickets will
//! use to build model dependency graphs and deterministic migration plans.
//! The current ticket intentionally keeps this crate minimal and compile-safe,
//! without introducing behavior changes in existing scan/watch/report flows.
//!
//! # Scope for this ticket
//!
//! - Provide a Rust 2024 workspace-integrated crate.
//! - Expose lightweight typed wrappers around a directed `petgraph` graph.
//! - Offer a small builder surface that downstream tickets can extend.

#![deny(clippy::all)]
#![warn(missing_docs)]

pub mod types;

use petgraph::Directed;
use petgraph::graph::{Graph, NodeIndex};

pub use types::{AstRelationEvidence, CstAnchor, EdgeKind, SourceClassification};

/// Strongly-typed directed graph alias used by graph-planning tickets.
///
/// The `u32` index type keeps the graph compact while still supporting large
/// projects.
pub type DependencyGraph<Node, Edge> = Graph<Node, Edge, Directed, u32>;

/// Builder for constructing a typed dependency graph.
///
/// This builder intentionally focuses on zero-cost wrappers around `petgraph`
/// and does not include planner logic yet.
#[derive(Debug, Clone)]
pub struct DependencyGraphBuilder<Node, Edge> {
    graph: DependencyGraph<Node, Edge>,
}

impl<Node, Edge> Default for DependencyGraphBuilder<Node, Edge> {
    fn default() -> Self {
        Self { graph: Graph::new() }
    }
}

impl<Node, Edge> DependencyGraphBuilder<Node, Edge> {
    /// Creates a new empty graph builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node to the graph and returns its index.
    pub fn add_node(&mut self, node: Node) -> NodeIndex<u32> {
        self.graph.add_node(node)
    }

    /// Adds a directed edge between two nodes.
    pub fn add_edge(&mut self, source: NodeIndex<u32>, target: NodeIndex<u32>, edge: Edge) {
        self.graph.add_edge(source, target, edge);
    }

    /// Returns an immutable view of the current graph.
    #[must_use]
    pub fn graph(&self) -> &DependencyGraph<Node, Edge> {
        &self.graph
    }

    /// Consumes the builder and returns the assembled graph.
    #[must_use]
    pub fn into_graph(self) -> DependencyGraph<Node, Edge> {
        self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AstRelationEvidence, DependencyGraph, DependencyGraphBuilder, EdgeKind,
        SourceClassification,
    };
    use crate::types::{ModelReference, ModelSource};
    use ch_core::ModelCategory;

    #[test]
    fn smoke_builder_constructs_and_exports_graph() {
        let mut builder = DependencyGraphBuilder::<&str, &str>::new();

        let legacy = builder.add_node("legacy_model");
        let modern = builder.add_node("modern_model");
        builder.add_edge(legacy, modern, "maps_to");

        let graph: DependencyGraph<_, _> = builder.into_graph();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn smoke_reexported_contract_types_are_constructible() {
        let source =
            ModelReference::new("LegacyOrder", ModelCategory::Model, ModelSource::SharedLegacy);
        let target = ModelReference::new("Order", ModelCategory::Model, ModelSource::Shared2023);
        let evidence = AstRelationEvidence::new(EdgeKind::LegacyBridge, source, target);

        assert_eq!(evidence.relation, EdgeKind::LegacyBridge);
        assert!(SourceClassification::Unknown.is_unknown());
    }
}
