//! Dependency graph foundations for migration planning.
//!
//! This crate exposes deterministic graph-building primitives used by migration
//! comparator and planner stages.

#![deny(clippy::all)]
#![warn(missing_docs)]

pub mod builder;
pub mod comparator;
pub mod export;
pub mod graph;
pub mod inventory;
pub mod mapping;
pub mod planner;
pub mod types;

pub use builder::DependencyGraphBuilder;
pub use comparator::GraphComparator;
pub use export::{export_artifacts, ExportError, GraphArtifactFormat, GraphArtifactSnapshotMode};
pub use graph::{
    DependencyGraph, DependencyStableGraph, GraphCounts, GraphEdge, GraphEdgeKindCount,
    GraphMetadata, GraphNode, GraphNodeKind, ParserMetadata,
};
pub use inventory::{
    build_inventory, CanonicalFallbackHit, InventoryAmbiguity, InventorySlotKind, ModelInventory,
    ModelInventoryBuilder, ModelInventoryRecord,
};
pub use mapping::{
    ComparatorConfig, GraphDiff, GraphDiffCounts, LegacyResidual, MappingReason, MappingReasonKind,
    MappingStatus, MappingWeights, ModelMapping,
};
pub use planner::{
    EvidenceRef, GraphPlanner, MigrationPlan, MigrationPlanCounts, MigrationStep, PlannerConfig,
    PlannerRiskWeights, RiskBreakdown, RiskComponentScore, RiskSignalKind, SuggestedReplacement,
    MAX_RISK_BPS,
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
        let source = ModelReference::new(
            "LegacyOrder",
            ModelCategory::Model,
            ModelSource::SharedLegacy,
        );
        let target = ModelReference::new("Order", ModelCategory::Model, ModelSource::Shared2023);
        let evidence = AstRelationEvidence::new(EdgeKind::LegacyBridge, source, target);

        assert_eq!(evidence.relation, EdgeKind::LegacyBridge);
        assert!(SourceClassification::Unknown.is_unknown());
    }
}
