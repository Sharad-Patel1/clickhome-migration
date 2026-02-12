//! Dependency graph foundations for migration planning.
//!
//! This crate exposes deterministic graph-building primitives used by migration
//! comparator and planner stages.

#![deny(clippy::all)]
#![warn(missing_docs)]

pub mod builder;
pub mod graph;
pub mod inventory;
pub mod types;

pub use builder::DependencyGraphBuilder;
pub use graph::{
    DependencyGraph, DependencyStableGraph, GraphCounts, GraphEdge, GraphEdgeKindCount,
    GraphMetadata, GraphNode, GraphNodeKind, ParserMetadata,
};
pub use inventory::{
    CanonicalFallbackHit, InventoryAmbiguity, InventorySlotKind, ModelInventory,
    ModelInventoryBuilder, ModelInventoryRecord, build_inventory,
};
pub use types::{
    AstRelationEvidence, CstAnchor, EdgeKind, ModelArtifact, ModelReference, ModelSource,
    SourceClassification,
};

#[cfg(test)]
mod tests {
    use super::{AstRelationEvidence, EdgeKind, SourceClassification};
    use crate::types::{ModelReference, ModelSource};
    use ch_core::ModelCategory;

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
