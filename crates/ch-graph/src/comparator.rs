//! Deterministic old-vs-new model comparator.
//!
//! This module maps legacy inventory records to modern equivalents with
//! confidence scoring and emits residual legacy usages where modern equivalents
//! are available.

use std::cmp::Ordering;

use ch_core::{
    AstRelationEvidence, CstAnchor, FileInfo, FxHashMap, FxHashSet, ModelArtifact, ModelCategory,
    ModelReference, ModelSource,
};
use ch_ts_parser::pascal_to_kebab;
use smallvec::SmallVec;

use crate::inventory::{ModelInventory, ModelInventoryRecord};
use crate::mapping::{
    ComparatorConfig, GraphDiff, LegacyResidual, MAX_CONFIDENCE_BPS, MappingReason,
    MappingReasonKind, MappingStatus, ModelMapping,
};

/// Deterministic comparator for legacy-to-modern model mapping.
#[derive(Debug, Clone, Default)]
pub struct GraphComparator {
    config: ComparatorConfig,
}

impl GraphComparator {
    /// Creates a comparator using [`ComparatorConfig::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a comparator with explicit config.
    #[must_use]
    pub const fn with_config(config: ComparatorConfig) -> Self {
        Self { config }
    }

    /// Returns the active comparator config.
    #[must_use]
    pub const fn config(&self) -> &ComparatorConfig {
        &self.config
    }

    /// Compares legacy and modern inventory records and emits deterministic diff
    /// outputs.
    #[must_use]
    pub fn compare(&self, inventory: &ModelInventory, files: &[FileInfo]) -> GraphDiff {
        let (match_threshold_bps, low_threshold_bps) = normalized_thresholds(&self.config);
        let sorted_files = sorted_files(files);
        let neighborhoods = build_symbol_neighborhoods(&sorted_files);
        let (legacy_shapes, modern_shapes, legacy_stem_lookup) =
            build_record_shapes(inventory, &neighborhoods);

        let mut mappings = Vec::with_capacity(legacy_shapes.len());
        for legacy in &legacy_shapes {
            let best = select_best_candidate(legacy, &modern_shapes, &self.config);
            mappings.push(build_model_mapping(
                legacy,
                best.as_ref(),
                match_threshold_bps,
                low_threshold_bps,
            ));
        }
        mappings.sort_by(compare_mapping);

        let mut residuals = build_residuals(&sorted_files, &mappings, &legacy_stem_lookup);
        residuals.sort_by(compare_residual);

        GraphDiff::new(mappings, residuals)
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

    fn from_artifact(artifact: &ModelArtifact) -> Self {
        Self {
            source: artifact.source,
            category: artifact.category,
            name: artifact.export_name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct RecordShape {
    canonical_id: String,
    primary_symbol: ModelReference,
    export_names: FxHashSet<String>,
    interface_stems: FxHashSet<String>,
    neighborhood_stems: FxHashSet<String>,
    symbol_stems: FxHashSet<String>,
}

#[derive(Debug, Clone)]
struct CandidateScore {
    modern_canonical_id: String,
    modern_symbol: ModelReference,
    confidence_bps: u16,
    reasons: SmallVec<[MappingReason; 4]>,
}

#[derive(Debug, Clone)]
struct ResidualTarget {
    modern_symbol: ModelReference,
    confidence_bps: u16,
    reasons: SmallVec<[MappingReason; 4]>,
}

fn normalized_thresholds(config: &ComparatorConfig) -> (u16, u16) {
    let match_threshold = config.match_threshold_bps.min(MAX_CONFIDENCE_BPS);
    let low_threshold = config.low_confidence_threshold_bps.min(match_threshold);
    (match_threshold, low_threshold)
}

fn sorted_files(files: &[FileInfo]) -> Vec<&FileInfo> {
    let mut sorted_files: Vec<&FileInfo> = files.iter().collect();
    sorted_files.sort_by(|left, right| {
        left.path.as_str().cmp(right.path.as_str()).then_with(|| left.id.0.cmp(&right.id.0))
    });
    sorted_files
}

fn build_symbol_neighborhoods(files: &[&FileInfo]) -> FxHashMap<SymbolKey, FxHashSet<String>> {
    let mut neighborhoods: FxHashMap<SymbolKey, FxHashSet<String>> = FxHashMap::default();

    for file in files {
        let mut refs: Vec<&ModelReference> = file.model_refs.iter().collect();
        refs.sort_by(|left, right| compare_model_reference(left, right));

        for (index, model_ref) in refs.iter().enumerate() {
            let key = SymbolKey::from_model_reference(model_ref);
            let entry = neighborhoods.entry(key).or_default();
            for (other_index, other_ref) in refs.iter().enumerate() {
                if index == other_index {
                    continue;
                }
                entry.insert(symbol_stem(other_ref.name.as_str()));
            }
        }

        let mut relations: Vec<&AstRelationEvidence> = file.relation_evidence.iter().collect();
        relations.sort_by(|left, right| compare_relation_evidence(left, right));
        for relation in relations {
            let source_key = SymbolKey::from_model_reference(&relation.source);
            let target_key = SymbolKey::from_model_reference(&relation.target);
            let source_stem = symbol_stem(relation.source.name.as_str());
            let target_stem = symbol_stem(relation.target.name.as_str());
            neighborhoods.entry(source_key).or_default().insert(target_stem);
            neighborhoods.entry(target_key).or_default().insert(source_stem);
        }
    }

    neighborhoods
}

fn build_record_shapes(
    inventory: &ModelInventory,
    neighborhoods: &FxHashMap<SymbolKey, FxHashSet<String>>,
) -> (Vec<RecordShape>, Vec<RecordShape>, FxHashMap<String, String>) {
    let mut legacy_shapes = Vec::new();
    let mut modern_shapes = Vec::new();
    let mut legacy_stem_lookup_raw: FxHashMap<String, Option<String>> = FxHashMap::default();

    for record in inventory.records() {
        let Some(shape) = record_shape(record, neighborhoods) else {
            continue;
        };

        match record.source {
            ModelSource::SharedLegacy => {
                for stem in &shape.symbol_stems {
                    update_stem_lookup(&mut legacy_stem_lookup_raw, stem, &shape.canonical_id);
                }
                legacy_shapes.push(shape);
            }
            ModelSource::Shared2023 => modern_shapes.push(shape),
            _ => {}
        }
    }

    legacy_shapes.sort_by(compare_record_shape);
    modern_shapes.sort_by(compare_record_shape);

    let mut legacy_stem_lookup = FxHashMap::default();
    for (stem, canonical_id) in legacy_stem_lookup_raw {
        if let Some(canonical_id) = canonical_id {
            legacy_stem_lookup.insert(stem, canonical_id);
        }
    }

    (legacy_shapes, modern_shapes, legacy_stem_lookup)
}

fn update_stem_lookup(
    lookup: &mut FxHashMap<String, Option<String>>,
    stem: &str,
    canonical_id: &str,
) {
    match lookup.entry(stem.to_owned()) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Some(canonical_id.to_owned()));
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            let previous = entry.get().clone();
            if previous.as_deref() != Some(canonical_id) {
                entry.insert(None);
            }
        }
    }
}

fn record_shape(
    record: &ModelInventoryRecord,
    neighborhoods: &FxHashMap<SymbolKey, FxHashSet<String>>,
) -> Option<RecordShape> {
    let artifacts = sorted_artifacts(record);
    if artifacts.is_empty() {
        return None;
    }

    let primary = primary_artifact(&artifacts)?;
    let primary_symbol =
        ModelReference::new(primary.export_name.clone(), primary.category, primary.source);

    let mut export_names: FxHashSet<String> = FxHashSet::default();
    let mut interface_stems: FxHashSet<String> = FxHashSet::default();
    let mut neighborhood_stems: FxHashSet<String> = FxHashSet::default();
    let mut symbol_stems: FxHashSet<String> = FxHashSet::default();

    for artifact in &artifacts {
        export_names.insert(artifact.export_name.clone());
        let stem = symbol_stem(artifact.export_name.as_str());
        symbol_stems.insert(stem.clone());

        if artifact.category == ModelCategory::Interface {
            interface_stems.insert(stem);
        }

        if let Some(neighbors) = neighborhoods.get(&SymbolKey::from_artifact(artifact)) {
            neighborhood_stems.extend(neighbors.iter().cloned());
        }
    }

    if interface_stems.is_empty() {
        interface_stems.insert(symbol_stem(primary_symbol.name.as_str()));
    }

    Some(RecordShape {
        canonical_id: record.canonical_id.clone(),
        primary_symbol,
        export_names,
        interface_stems,
        neighborhood_stems,
        symbol_stems,
    })
}

fn sorted_artifacts(record: &ModelInventoryRecord) -> Vec<&ModelArtifact> {
    let mut artifacts = Vec::new();
    if let Some(artifact) = record.interface.as_ref() {
        artifacts.push(artifact);
    }
    if let Some(artifact) = record.codegen_interface.as_ref() {
        artifacts.push(artifact);
    }
    if let Some(artifact) = record.wrapper.as_ref() {
        artifacts.push(artifact);
    }
    if let Some(artifact) = record.codegen.as_ref() {
        artifacts.push(artifact);
    }
    if let Some(artifact) = record.service_wrapper.as_ref() {
        artifacts.push(artifact);
    }
    if let Some(artifact) = record.service_codegen.as_ref() {
        artifacts.push(artifact);
    }

    artifacts.sort_by(|left, right| {
        category_order(left.category)
            .cmp(&category_order(right.category))
            .then_with(|| left.export_name.cmp(&right.export_name))
            .then_with(|| left.definition_path.as_str().cmp(right.definition_path.as_str()))
    });
    artifacts
}

fn primary_artifact<'a>(artifacts: &'a [&ModelArtifact]) -> Option<&'a ModelArtifact> {
    artifacts.iter().copied().min_by(|left, right| {
        category_order(left.category)
            .cmp(&category_order(right.category))
            .then_with(|| left.export_name.cmp(&right.export_name))
            .then_with(|| left.definition_path.as_str().cmp(right.definition_path.as_str()))
    })
}

fn select_best_candidate(
    legacy: &RecordShape,
    modern_shapes: &[RecordShape],
    config: &ComparatorConfig,
) -> Option<CandidateScore> {
    let mut best: Option<CandidateScore> = None;

    for modern in modern_shapes {
        let candidate = evaluate_candidate(legacy, modern, config);
        match &best {
            None => best = Some(candidate),
            Some(current) => {
                if compare_candidate_score(&candidate, current) == Ordering::Less {
                    best = Some(candidate);
                }
            }
        }
    }

    best
}

fn compare_candidate_score(left: &CandidateScore, right: &CandidateScore) -> Ordering {
    right
        .confidence_bps
        .cmp(&left.confidence_bps)
        .then_with(|| left.modern_canonical_id.cmp(&right.modern_canonical_id))
        .then_with(|| compare_model_reference(&left.modern_symbol, &right.modern_symbol))
}

fn evaluate_candidate(
    legacy: &RecordShape,
    modern: &RecordShape,
    config: &ComparatorConfig,
) -> CandidateScore {
    let mut reasons: SmallVec<[MappingReason; 4]> = SmallVec::new();
    let mut confidence: u32 = 0;

    if legacy.canonical_id == modern.canonical_id {
        let weight = config.weights.canonical_exact_bps;
        confidence = confidence.saturating_add(u32::from(weight));
        reasons.push(MappingReason::new(
            MappingReasonKind::CanonicalIdExact,
            weight,
            format!("canonical_id={}", legacy.canonical_id),
        ));
    }

    if sets_intersect(&legacy.interface_stems, &modern.interface_stems) {
        let weight = config.weights.interface_equivalence_bps;
        confidence = confidence.saturating_add(u32::from(weight));
        reasons.push(MappingReason::new(
            MappingReasonKind::InterfaceNameEquivalent,
            weight,
            format!(
                "legacy_interfaces={},modern_interfaces={}",
                legacy.interface_stems.len(),
                modern.interface_stems.len()
            ),
        ));
    }

    let (export_intersection, export_union, export_overlap_bps) =
        overlap_ratio_bps(&legacy.export_names, &modern.export_names);
    let export_weight = weighted_overlap(config.weights.export_overlap_bps, export_overlap_bps);
    if export_weight > 0 {
        confidence = confidence.saturating_add(u32::from(export_weight));
        reasons.push(MappingReason::new(
            MappingReasonKind::ExportOverlap,
            export_weight,
            format!("intersection={export_intersection},union={export_union}"),
        ));
    }

    let (neighborhood_intersection, neighborhood_union, neighborhood_overlap_bps) =
        overlap_ratio_bps(&legacy.neighborhood_stems, &modern.neighborhood_stems);
    let neighborhood_weight =
        weighted_overlap(config.weights.usage_neighborhood_bps, neighborhood_overlap_bps);
    if neighborhood_weight > 0 {
        confidence = confidence.saturating_add(u32::from(neighborhood_weight));
        reasons.push(MappingReason::new(
            MappingReasonKind::UsageNeighborhoodOverlap,
            neighborhood_weight,
            format!("intersection={neighborhood_intersection},union={neighborhood_union}"),
        ));
    }

    let bounded_confidence = confidence.min(u32::from(MAX_CONFIDENCE_BPS));
    let confidence_bps = u16::try_from(bounded_confidence).ok().unwrap_or(MAX_CONFIDENCE_BPS);

    CandidateScore {
        modern_canonical_id: modern.canonical_id.clone(),
        modern_symbol: modern.primary_symbol.clone(),
        confidence_bps,
        reasons,
    }
}

fn build_model_mapping(
    legacy: &RecordShape,
    best: Option<&CandidateScore>,
    match_threshold_bps: u16,
    low_threshold_bps: u16,
) -> ModelMapping {
    let (confidence_bps, reasons) = best
        .map(|candidate| (candidate.confidence_bps, candidate.reasons.clone()))
        .unwrap_or_default();

    let status = mapping_status(confidence_bps, match_threshold_bps, low_threshold_bps);
    let (modern_canonical_id, modern_symbol) = if status.has_target() {
        best.map_or((None, None), |candidate| {
            (Some(candidate.modern_canonical_id.clone()), Some(candidate.modern_symbol.clone()))
        })
    } else {
        (None, None)
    };

    ModelMapping {
        legacy_canonical_id: legacy.canonical_id.clone(),
        legacy_symbol: legacy.primary_symbol.clone(),
        modern_canonical_id,
        modern_symbol,
        confidence_bps,
        status,
        reasons,
    }
}

fn mapping_status(
    confidence_bps: u16,
    match_threshold_bps: u16,
    low_threshold_bps: u16,
) -> MappingStatus {
    if confidence_bps >= match_threshold_bps {
        MappingStatus::Matched
    } else if confidence_bps >= low_threshold_bps {
        MappingStatus::LowConfidence
    } else {
        MappingStatus::NoMatch
    }
}

fn build_residuals(
    files: &[&FileInfo],
    mappings: &[ModelMapping],
    legacy_stem_lookup: &FxHashMap<String, String>,
) -> Vec<LegacyResidual> {
    let mut targets_by_canonical: FxHashMap<String, ResidualTarget> = FxHashMap::default();
    for mapping in mappings {
        let Some(modern_symbol) = mapping.modern_symbol.as_ref() else {
            continue;
        };
        targets_by_canonical.insert(
            mapping.legacy_canonical_id.clone(),
            ResidualTarget {
                modern_symbol: modern_symbol.clone(),
                confidence_bps: mapping.confidence_bps,
                reasons: mapping.reasons.clone(),
            },
        );
    }

    let mut residuals = Vec::new();
    for file in files {
        let mut legacy_refs: Vec<&ModelReference> =
            file.model_refs.iter().filter(|model_ref| model_ref.is_legacy()).collect();
        legacy_refs.sort_by(|left, right| compare_model_reference(left, right));

        for legacy_ref in legacy_refs {
            let canonical_id = canonical_for_legacy_ref(legacy_ref, legacy_stem_lookup);
            let Some(target) = targets_by_canonical.get(canonical_id.as_str()) else {
                continue;
            };

            residuals.push(LegacyResidual {
                file_path: file.path.clone(),
                legacy_symbol: legacy_ref.clone(),
                suggested_modern_symbol: target.modern_symbol.clone(),
                confidence_bps: target.confidence_bps,
                reasons: target.reasons.clone(),
                anchors: collect_residual_anchors(file, legacy_ref),
            });
        }
    }

    residuals
}

fn canonical_for_legacy_ref(
    legacy_ref: &ModelReference,
    legacy_stem_lookup: &FxHashMap<String, String>,
) -> String {
    let stem = symbol_stem(legacy_ref.name.as_str());
    if let Some(canonical_id) = legacy_stem_lookup.get(stem.as_str()) {
        return canonical_id.clone();
    }
    pascal_to_kebab(stem.as_str())
}

fn collect_residual_anchors(
    file: &FileInfo,
    legacy_ref: &ModelReference,
) -> SmallVec<[CstAnchor; 2]> {
    let mut anchors: FxHashSet<CstAnchor> = FxHashSet::default();

    let mut relations: Vec<&AstRelationEvidence> = file.relation_evidence.iter().collect();
    relations.sort_by(|left, right| compare_relation_evidence(left, right));
    for relation in relations {
        if relation.source == *legacy_ref || relation.target == *legacy_ref {
            anchors.extend(relation.anchors.iter().cloned());
        }
    }

    let mut anchors: SmallVec<[CstAnchor; 2]> = anchors.into_iter().collect();
    anchors.sort_by(compare_anchor);
    anchors
}

fn compare_mapping(left: &ModelMapping, right: &ModelMapping) -> Ordering {
    left.legacy_canonical_id
        .cmp(&right.legacy_canonical_id)
        .then_with(|| compare_model_reference(&left.legacy_symbol, &right.legacy_symbol))
}

fn compare_residual(left: &LegacyResidual, right: &LegacyResidual) -> Ordering {
    left.file_path
        .as_str()
        .cmp(right.file_path.as_str())
        .then_with(|| compare_model_reference(&left.legacy_symbol, &right.legacy_symbol))
        .then_with(|| {
            compare_model_reference(&left.suggested_modern_symbol, &right.suggested_modern_symbol)
        })
}

fn compare_relation_evidence(left: &AstRelationEvidence, right: &AstRelationEvidence) -> Ordering {
    edge_kind_order(left.relation)
        .cmp(&edge_kind_order(right.relation))
        .then_with(|| compare_model_reference(&left.source, &right.source))
        .then_with(|| compare_model_reference(&left.target, &right.target))
        .then_with(|| left.anchors.len().cmp(&right.anchors.len()))
}

fn compare_anchor(left: &CstAnchor, right: &CstAnchor) -> Ordering {
    left.file_path
        .as_str()
        .cmp(right.file_path.as_str())
        .then_with(|| left.start_byte.cmp(&right.start_byte))
        .then_with(|| left.end_byte.cmp(&right.end_byte))
        .then_with(|| left.start.line.cmp(&right.start.line))
        .then_with(|| left.start.column.cmp(&right.start.column))
        .then_with(|| left.start.byte_offset.cmp(&right.start.byte_offset))
        .then_with(|| left.end.line.cmp(&right.end.line))
        .then_with(|| left.end.column.cmp(&right.end.column))
        .then_with(|| left.end.byte_offset.cmp(&right.end.byte_offset))
        .then_with(|| left.node_kind.cmp(&right.node_kind))
        .then_with(|| compare_optional_str(left.field_name.as_deref(), right.field_name.as_deref()))
        .then_with(|| {
            import_kind_order(left.import_kind).cmp(&import_kind_order(right.import_kind))
        })
        .then_with(|| left.is_type_only.cmp(&right.is_type_only))
        .then_with(|| left.snippet_hash.cmp(&right.snippet_hash))
}

fn compare_optional_str(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => left.cmp(right),
    }
}

fn compare_record_shape(left: &RecordShape, right: &RecordShape) -> Ordering {
    left.canonical_id
        .cmp(&right.canonical_id)
        .then_with(|| compare_model_reference(&left.primary_symbol, &right.primary_symbol))
}

fn compare_model_reference(left: &ModelReference, right: &ModelReference) -> Ordering {
    source_order(left.source)
        .cmp(&source_order(right.source))
        .then_with(|| category_order(left.category).cmp(&category_order(right.category)))
        .then_with(|| left.name.cmp(&right.name))
}

fn sets_intersect(left: &FxHashSet<String>, right: &FxHashSet<String>) -> bool {
    if left.len() <= right.len() {
        left.iter().any(|item| right.contains(item))
    } else {
        right.iter().any(|item| left.contains(item))
    }
}

fn overlap_ratio_bps(left: &FxHashSet<String>, right: &FxHashSet<String>) -> (usize, usize, u16) {
    if left.is_empty() && right.is_empty() {
        return (0, 0, 0);
    }

    let intersection = if left.len() <= right.len() {
        left.iter().filter(|item| right.contains(*item)).count()
    } else {
        right.iter().filter(|item| left.contains(*item)).count()
    };
    let union = left.len().saturating_add(right.len()).saturating_sub(intersection);

    if union == 0 {
        return (intersection, union, 0);
    }

    let ratio = (intersection * usize::from(MAX_CONFIDENCE_BPS)) / union;
    let ratio = u16::try_from(ratio).ok().unwrap_or(MAX_CONFIDENCE_BPS);
    (intersection, union, ratio)
}

fn weighted_overlap(max_weight_bps: u16, overlap_ratio_bps: u16) -> u16 {
    let weighted =
        (u32::from(max_weight_bps) * u32::from(overlap_ratio_bps)) / u32::from(MAX_CONFIDENCE_BPS);
    u16::try_from(weighted).ok().unwrap_or(max_weight_bps)
}

fn symbol_stem(symbol_name: &str) -> String {
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
                return base.to_owned();
            }
        }
    }
    symbol_name.to_owned()
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

const fn edge_kind_order(kind: ch_core::EdgeKind) -> u8 {
    match kind {
        ch_core::EdgeKind::ImportsSymbol => 0,
        ch_core::EdgeKind::ReexportsSymbol => 1,
        ch_core::EdgeKind::TypeRef => 2,
        ch_core::EdgeKind::Extends => 3,
        ch_core::EdgeKind::Implements => 4,
        ch_core::EdgeKind::Constructs => 5,
        ch_core::EdgeKind::FactoryCall => 6,
        ch_core::EdgeKind::ServiceParamType => 7,
        ch_core::EdgeKind::ServiceReturnType => 8,
        ch_core::EdgeKind::ModelMapRegistration => 9,
        ch_core::EdgeKind::LegacyBridge => 10,
        ch_core::EdgeKind::GeneratedFrom => 11,
        _ => u8::MAX,
    }
}

const fn import_kind_order(kind: Option<ch_core::ImportKind>) -> u8 {
    match kind {
        None => 0,
        Some(ch_core::ImportKind::Named) => 1,
        Some(ch_core::ImportKind::Default) => 2,
        Some(ch_core::ImportKind::Namespace) => 3,
        Some(ch_core::ImportKind::TypeOnly) => 4,
        _ => u8::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::build_inventory;
    use ch_core::{FileId, MigrationStatus, ModelDefinition, ModelRegistry, SourceLocation};
    use smallvec::smallvec;

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

    fn exact_match_inventory() -> ModelInventory {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "OrderModel",
            ModelSource::SharedLegacy,
            "shared/interfaces.ts",
            &["OrderModel"],
        ));
        registry.register(definition(
            "OrderCodeGen",
            ModelSource::SharedLegacy,
            "shared/interfaces.codegen.ts",
            &["OrderCodeGen"],
        ));
        registry.register(definition(
            "Order",
            ModelSource::SharedLegacy,
            "shared/models/order.ts",
            &["Order", "OrderCodeGen", "OrderService", "OrderServiceCodeGen"],
        ));
        registry.register(definition(
            "OrderModel",
            ModelSource::Shared2023,
            "shared_2023/interfaces.ts",
            &["OrderModel"],
        ));
        registry.register(definition(
            "OrderCodeGen",
            ModelSource::Shared2023,
            "shared_2023/interfaces.codegen.ts",
            &["OrderCodeGen"],
        ));
        registry.register(definition(
            "Order",
            ModelSource::Shared2023,
            "shared_2023/models/order.ts",
            &["Order", "OrderCodeGen", "OrderService", "OrderServiceCodeGen"],
        ));
        build_inventory(&registry)
    }

    fn renamed_inventory() -> ModelInventory {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "LegacyOrderModel",
            ModelSource::SharedLegacy,
            "shared/interfaces.ts",
            &["LegacyOrderModel"],
        ));
        registry.register(definition(
            "LegacyOrder",
            ModelSource::SharedLegacy,
            "shared/models/legacy-order.ts",
            &["LegacyOrder", "LegacyOrderCodeGen"],
        ));
        registry.register(definition(
            "OrderModel",
            ModelSource::Shared2023,
            "shared_2023/interfaces.ts",
            &["OrderModel"],
        ));
        registry.register(definition(
            "Order",
            ModelSource::Shared2023,
            "shared_2023/models/order.ts",
            &["Order", "OrderCodeGen"],
        ));
        build_inventory(&registry)
    }

    fn renamed_files() -> Vec<FileInfo> {
        let legacy_order = ModelReference::new(
            "LegacyOrderModel",
            ModelCategory::Interface,
            ModelSource::SharedLegacy,
        );
        let modern_order =
            ModelReference::new("OrderModel", ModelCategory::Interface, ModelSource::Shared2023);
        let legacy_customer = ModelReference::new(
            "CustomerModel",
            ModelCategory::Interface,
            ModelSource::SharedLegacy,
        );
        let modern_customer =
            ModelReference::new("CustomerModel", ModelCategory::Interface, ModelSource::Shared2023);

        let mut legacy_file = FileInfo::new(FileId::new(11), "src/legacy.ts".into());
        legacy_file.status = MigrationStatus::Legacy;
        legacy_file.model_refs = smallvec![legacy_order, legacy_customer];

        let mut modern_file = FileInfo::new(FileId::new(12), "src/modern.ts".into());
        modern_file.status = MigrationStatus::Migrated;
        modern_file.model_refs = smallvec![modern_order, modern_customer];

        vec![legacy_file, modern_file]
    }

    #[test]
    fn test_compare_exact_match_with_reasons() {
        let inventory = exact_match_inventory();
        let diff = GraphComparator::new().compare(&inventory, &[]);

        assert_eq!(diff.mappings.len(), 1);
        let mapping = &diff.mappings[0];
        assert_eq!(mapping.legacy_canonical_id, "order");
        assert_eq!(mapping.status, MappingStatus::Matched);
        assert!(mapping.modern_symbol.is_some());
        assert!(
            mapping.reasons.iter().any(|reason| reason.kind == MappingReasonKind::CanonicalIdExact)
        );
    }

    #[test]
    fn test_compare_renamed_mapping_with_usage_fallback() {
        let inventory = renamed_inventory();
        let files = renamed_files();

        let config = ComparatorConfig {
            match_threshold_bps: 90,
            low_confidence_threshold_bps: 50,
            ..ComparatorConfig::default()
        };

        let diff = GraphComparator::with_config(config).compare(&inventory, &files);
        assert_eq!(diff.mappings.len(), 1);
        let mapping = &diff.mappings[0];
        assert_eq!(mapping.legacy_canonical_id, "legacy-order");
        assert_eq!(mapping.status, MappingStatus::Matched);
        assert!(mapping.modern_symbol.is_some());
        assert!(
            !mapping
                .reasons
                .iter()
                .any(|reason| reason.kind == MappingReasonKind::CanonicalIdExact)
        );
        assert!(
            mapping
                .reasons
                .iter()
                .any(|reason| reason.kind == MappingReasonKind::UsageNeighborhoodOverlap)
        );
    }

    #[test]
    fn test_compare_no_match_and_low_confidence_thresholds() {
        let inventory = renamed_inventory();
        let files = renamed_files();

        let no_match = GraphComparator::new().compare(&inventory, &files);
        assert_eq!(no_match.mappings[0].status, MappingStatus::NoMatch);
        assert!(no_match.mappings[0].modern_symbol.is_none());

        let low_conf_config = ComparatorConfig {
            match_threshold_bps: 700,
            low_confidence_threshold_bps: 50,
            ..ComparatorConfig::default()
        };
        let low_conf = GraphComparator::with_config(low_conf_config).compare(&inventory, &files);
        assert_eq!(low_conf.mappings[0].status, MappingStatus::LowConfidence);
        assert!(low_conf.mappings[0].modern_symbol.is_some());
    }

    #[test]
    fn test_compare_emits_residual_legacy_usage() {
        let inventory = exact_match_inventory();
        let legacy_order =
            ModelReference::new("OrderModel", ModelCategory::Interface, ModelSource::SharedLegacy);

        let mut file = FileInfo::new(FileId::new(21), "src/order.ts".into());
        file.status = MigrationStatus::Legacy;
        file.model_refs = smallvec![legacy_order];

        let diff = GraphComparator::new().compare(&inventory, &[file]);
        assert_eq!(diff.residual_legacy_usages.len(), 1);

        let residual = &diff.residual_legacy_usages[0];
        assert_eq!(residual.file_path.as_str(), "src/order.ts");
        assert_eq!(residual.legacy_symbol.name, "OrderModel");
        assert_eq!(residual.suggested_modern_symbol.name, "OrderModel");
    }

    #[test]
    fn test_compare_is_deterministic_across_file_order() {
        let inventory = exact_match_inventory();
        let legacy_order =
            ModelReference::new("OrderModel", ModelCategory::Interface, ModelSource::SharedLegacy);
        let modern_order =
            ModelReference::new("OrderModel", ModelCategory::Interface, ModelSource::Shared2023);
        let mut relation = AstRelationEvidence::new(
            ch_core::EdgeKind::LegacyBridge,
            legacy_order.clone(),
            modern_order.clone(),
        );
        relation.add_anchor(CstAnchor::new(
            "src/a.ts",
            10,
            22,
            SourceLocation::new(1, 0, 10),
            SourceLocation::new(1, 12, 22),
            "identifier",
        ));

        let mut first = FileInfo::new(FileId::new(31), "src/a.ts".into());
        first.status = MigrationStatus::Partial;
        first.model_refs = smallvec![legacy_order.clone(), modern_order.clone()];
        first.relation_evidence = smallvec![relation];

        let mut second = FileInfo::new(FileId::new(32), "src/b.ts".into());
        second.status = MigrationStatus::Legacy;
        second.model_refs = smallvec![legacy_order];

        let diff_a = GraphComparator::new().compare(&inventory, &[first.clone(), second.clone()]);
        let diff_b = GraphComparator::new().compare(&inventory, &[second, first]);

        assert_eq!(diff_a, diff_b);
    }
}
