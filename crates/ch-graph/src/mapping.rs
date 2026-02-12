//! Comparator and diff contracts for old-vs-new model mapping.
//!
//! These types form the public output schema for graph comparison between the
//! legacy and modern model ecosystems.

use camino::Utf8PathBuf;
use ch_core::{CstAnchor, ModelReference};
use smallvec::SmallVec;

/// Maximum confidence score represented in basis points (`1000 == 100%`).
pub const MAX_CONFIDENCE_BPS: u16 = 1_000;

/// Weights for comparator signals in basis points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MappingWeights {
    /// Weight for exact canonical-ID matches.
    pub canonical_exact_bps: u16,
    /// Weight for interface-name equivalence.
    pub interface_equivalence_bps: u16,
    /// Maximum weight contributed by export-signature overlap.
    pub export_overlap_bps: u16,
    /// Maximum weight contributed by usage-neighborhood overlap.
    pub usage_neighborhood_bps: u16,
}

impl Default for MappingWeights {
    fn default() -> Self {
        Self {
            canonical_exact_bps: 450,
            interface_equivalence_bps: 250,
            export_overlap_bps: 200,
            usage_neighborhood_bps: 100,
        }
    }
}

impl MappingWeights {
    /// Returns the total configured weight in basis points.
    #[must_use]
    pub const fn total_bps(self) -> u16 {
        self.canonical_exact_bps
            .saturating_add(self.interface_equivalence_bps)
            .saturating_add(self.export_overlap_bps)
            .saturating_add(self.usage_neighborhood_bps)
    }
}

/// Comparator thresholds and signal weights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparatorConfig {
    /// Minimum confidence (inclusive) to classify as [`MappingStatus::Matched`].
    pub match_threshold_bps: u16,
    /// Minimum confidence (inclusive) to classify as
    /// [`MappingStatus::LowConfidence`].
    pub low_confidence_threshold_bps: u16,
    /// Signal weights used by the comparator.
    pub weights: MappingWeights,
}

impl Default for ComparatorConfig {
    fn default() -> Self {
        Self {
            match_threshold_bps: 700,
            low_confidence_threshold_bps: 500,
            weights: MappingWeights::default(),
        }
    }
}

/// Mapping outcome after evaluating confidence thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MappingStatus {
    /// Candidate modern equivalent satisfied the match threshold.
    Matched,
    /// Candidate modern equivalent exists but did not satisfy full match
    /// confidence.
    LowConfidence,
    /// No acceptable equivalent candidate.
    NoMatch,
}

impl MappingStatus {
    /// Returns `true` when this status has an equivalent modern target.
    #[must_use]
    pub const fn has_target(self) -> bool {
        !matches!(self, Self::NoMatch)
    }
}

/// The type of signal that contributed to mapping confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MappingReasonKind {
    /// Legacy and modern records share exact canonical IDs.
    CanonicalIdExact,
    /// Interface names are equivalent under canonical stem normalization.
    InterfaceNameEquivalent,
    /// Export-signature overlap contributed confidence.
    ExportOverlap,
    /// Usage-neighborhood overlap contributed confidence.
    UsageNeighborhoodOverlap,
}

/// One scored reason contributing to a mapping result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MappingReason {
    /// Signal class.
    pub kind: MappingReasonKind,
    /// Contribution in basis points.
    pub weight_bps: u16,
    /// Deterministic details string for diagnostics.
    pub details: String,
}

impl MappingReason {
    /// Creates a new mapping reason.
    #[must_use]
    pub fn new(kind: MappingReasonKind, weight_bps: u16, details: impl Into<String>) -> Self {
        Self {
            kind,
            weight_bps,
            details: details.into(),
        }
    }
}

/// Mapping decision for one legacy canonical model record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMapping {
    /// Legacy canonical identifier.
    pub legacy_canonical_id: String,
    /// Legacy representative symbol for this mapping.
    pub legacy_symbol: ModelReference,
    /// Modern canonical identifier when available.
    pub modern_canonical_id: Option<String>,
    /// Modern representative symbol when available.
    pub modern_symbol: Option<ModelReference>,
    /// Total confidence in basis points (`0..=1000`).
    pub confidence_bps: u16,
    /// Threshold-based mapping status.
    pub status: MappingStatus,
    /// Ordered reason list explaining confidence composition.
    pub reasons: SmallVec<[MappingReason; 4]>,
}

/// Residual legacy reference with an available modern equivalent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyResidual {
    /// Consumer file where the residual legacy usage was observed.
    pub file_path: Utf8PathBuf,
    /// Legacy symbol reference still in use.
    pub legacy_symbol: ModelReference,
    /// Suggested modern symbol reference.
    pub suggested_modern_symbol: ModelReference,
    /// Confidence copied from the source mapping decision.
    pub confidence_bps: u16,
    /// Ordered reason list copied from the source mapping decision.
    pub reasons: SmallVec<[MappingReason; 4]>,
    /// Optional relation anchors supporting this residual.
    pub anchors: SmallVec<[CstAnchor; 2]>,
}

/// Summary counters for a comparison run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GraphDiffCounts {
    /// Number of `Matched` mapping outcomes.
    pub matched: usize,
    /// Number of `LowConfidence` mapping outcomes.
    pub low_confidence: usize,
    /// Number of `NoMatch` mapping outcomes.
    pub no_match: usize,
    /// Number of emitted residual legacy usages.
    pub residuals: usize,
}

/// Full old-vs-new comparison output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphDiff {
    /// Deterministically ordered mapping decisions for legacy records.
    pub mappings: Vec<ModelMapping>,
    /// Deterministically ordered residual legacy usages.
    pub residual_legacy_usages: Vec<LegacyResidual>,
    /// Outcome counters derived from mapping/residual vectors.
    pub counts: GraphDiffCounts,
}

impl GraphDiff {
    /// Creates a graph diff and derives summary counters.
    #[must_use]
    pub fn new(mappings: Vec<ModelMapping>, residual_legacy_usages: Vec<LegacyResidual>) -> Self {
        let mut counts = GraphDiffCounts::default();
        for mapping in &mappings {
            match mapping.status {
                MappingStatus::Matched => counts.matched += 1,
                MappingStatus::LowConfidence => counts.low_confidence += 1,
                MappingStatus::NoMatch => counts.no_match += 1,
            }
        }
        counts.residuals = residual_legacy_usages.len();
        Self {
            mappings,
            residual_legacy_usages,
            counts,
        }
    }

    /// Returns `true` when both mappings and residuals are empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty() && self.residual_legacy_usages.is_empty()
    }
}
