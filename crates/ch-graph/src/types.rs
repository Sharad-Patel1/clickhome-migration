//! Graph contract types re-exported from `ch-core`.
//!
//! `ch-graph` is the public entry point for graph/planning consumers. This
//! module re-exports shared evidence contracts that are canonically defined in
//! `ch-core`.

pub use ch_core::{
    AstRelationEvidence, CstAnchor, EdgeKind, ModelArtifact, ModelReference, ModelSource,
    SourceClassification,
};
