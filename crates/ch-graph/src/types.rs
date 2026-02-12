//! Graph contract types re-exported from `ch-core`.
//!
//! `ch-graph` is the public entry point for graph/planning consumers. This
//! module re-exports shared evidence contracts that are canonically defined in
//! `ch-core`.

pub use ch_core::{
    AstRelationEvidence, CstAnchor, EdgeKind, ModelArtifact, ModelReference, ModelSource,
    SourceClassification,
};

pub use crate::graph::{
    DependencyGraph, DependencyStableGraph, GraphCounts, GraphEdge, GraphEdgeKindCount,
    GraphMetadata, GraphNode, GraphNodeKind, ParserMetadata,
};
pub use crate::mapping::{
    ComparatorConfig, GraphDiff, GraphDiffCounts, LegacyResidual, MappingReason, MappingReasonKind,
    MappingStatus, MappingWeights, ModelMapping,
};
pub use crate::planner::{
    EvidenceRef, GraphPlanner, MigrationPlan, MigrationPlanCounts, MigrationStep, PlannerConfig,
    PlannerRiskWeights, RiskBreakdown, RiskComponentScore, RiskSignalKind, SuggestedReplacement,
    MAX_RISK_BPS,
};
