//! Deterministic per-model inventory builder for graph planning.
//!
//! This module converts scanned [`ch_core::ModelRegistry`] entries into
//! canonical per-model records that downstream graph and comparator stages can
//! consume.

use camino::Utf8Path;
use ch_core::{FxHashMap, ModelArtifact, ModelCategory, ModelRegistry, ModelSource};
use ch_ts_parser::{kebab_to_pascal, pascal_to_kebab};

const DEFAULT_CANONICAL_ID_FALLBACKS: &[(&str, &str)] = &[
    ("HTMLReport", "html-report"),
    ("OAuthClient", "oauth-client"),
];

#[derive(Debug, Default)]
struct PendingInventoryRecord {
    interface: Vec<ModelArtifact>,
    codegen_interface: Vec<ModelArtifact>,
    wrapper: Vec<ModelArtifact>,
    codegen: Vec<ModelArtifact>,
    service_wrapper: Vec<ModelArtifact>,
    service_codegen: Vec<ModelArtifact>,
}

impl PendingInventoryRecord {
    fn slot_mut(&mut self, slot: InventorySlotKind) -> &mut Vec<ModelArtifact> {
        match slot {
            InventorySlotKind::Interface => &mut self.interface,
            InventorySlotKind::CodegenInterface => &mut self.codegen_interface,
            InventorySlotKind::Wrapper => &mut self.wrapper,
            InventorySlotKind::Codegen => &mut self.codegen,
            InventorySlotKind::ServiceWrapper => &mut self.service_wrapper,
            InventorySlotKind::ServiceCodegen => &mut self.service_codegen,
        }
    }

    fn push(&mut self, slot: InventorySlotKind, artifact: ModelArtifact) {
        let candidates = self.slot_mut(slot);
        if !candidates.contains(&artifact) {
            candidates.push(artifact);
        }
    }
}

/// Inventory slot representing a link target in a canonical model record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventorySlotKind {
    /// `interfaces.ts` declaration (`{Model}Model`).
    Interface,
    /// `interfaces.codegen.ts` declaration (`{Model}CodeGen`).
    CodegenInterface,
    /// Wrapper model class (typically `{Model}`).
    Wrapper,
    /// Codegen model class (`{Model}CodeGen`, `CodeGenForApi`, `CodeGenForm`, ...).
    Codegen,
    /// Service wrapper class (`{Model}Service`).
    ServiceWrapper,
    /// Service codegen class (`{Model}ServiceCodeGen`).
    ServiceCodegen,
}

/// Artifact candidate ambiguity for a canonical model slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryAmbiguity {
    /// Source ecosystem where ambiguity occurred.
    pub source: ModelSource,
    /// Canonical model identifier.
    pub canonical_id: String,
    /// Slot with multiple candidates.
    pub slot: InventorySlotKind,
    /// Candidate artifacts for the slot.
    pub candidates: Vec<ModelArtifact>,
}

/// Fallback table usage report entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalFallbackHit {
    /// Pascal-cased symbol key that matched a fallback entry.
    pub pascal_name: String,
    /// Canonical ID produced by the fallback.
    pub canonical_id: String,
    /// Number of times this fallback mapping was used.
    pub count: usize,
}

/// Canonical model inventory record for one `(source, canonical_id)` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInventoryRecord {
    /// Source ecosystem (legacy or modern).
    pub source: ModelSource,
    /// Deterministic canonical identifier (kebab-case).
    pub canonical_id: String,
    /// Interface artifact candidate.
    pub interface: Option<ModelArtifact>,
    /// Codegen interface artifact candidate.
    pub codegen_interface: Option<ModelArtifact>,
    /// Wrapper model artifact candidate.
    pub wrapper: Option<ModelArtifact>,
    /// Codegen model artifact candidate.
    pub codegen: Option<ModelArtifact>,
    /// Service wrapper artifact candidate.
    pub service_wrapper: Option<ModelArtifact>,
    /// Service codegen artifact candidate.
    pub service_codegen: Option<ModelArtifact>,
}

impl ModelInventoryRecord {
    /// Returns `true` when the interface->codegen->wrapper model chain is complete.
    #[must_use]
    pub const fn has_model_chain(&self) -> bool {
        self.interface.is_some()
            && (self.codegen_interface.is_some() || self.codegen.is_some())
            && self.wrapper.is_some()
    }

    /// Returns `true` when the service codegen->service wrapper chain is complete.
    #[must_use]
    pub const fn has_service_chain(&self) -> bool {
        self.service_codegen.is_some() && self.service_wrapper.is_some()
    }
}

/// Deterministic inventory output for graph/comparator stages.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelInventory {
    records: Vec<ModelInventoryRecord>,
    ambiguities: Vec<InventoryAmbiguity>,
    fallback_hits: Vec<CanonicalFallbackHit>,
}

impl ModelInventory {
    /// Returns all canonical inventory records in deterministic order.
    #[must_use]
    pub fn records(&self) -> &[ModelInventoryRecord] {
        &self.records
    }

    /// Returns an iterator over canonical inventory records.
    pub fn iter(&self) -> impl Iterator<Item = &ModelInventoryRecord> {
        self.records.iter()
    }

    /// Returns all ambiguity records in deterministic order.
    #[must_use]
    pub fn ambiguities(&self) -> &[InventoryAmbiguity] {
        &self.ambiguities
    }

    /// Returns fallback usage report entries in deterministic order.
    #[must_use]
    pub fn fallback_hits(&self) -> &[CanonicalFallbackHit] {
        &self.fallback_hits
    }

    /// Returns a canonical inventory record by `(source, canonical_id)`.
    #[must_use]
    pub fn find(&self, source: ModelSource, canonical_id: &str) -> Option<&ModelInventoryRecord> {
        self.records
            .iter()
            .find(|record| record.source == source && record.canonical_id == canonical_id)
    }
}

/// Builder for deterministic canonical model inventory.
#[derive(Debug, Clone)]
pub struct ModelInventoryBuilder {
    canonical_id_fallbacks: FxHashMap<String, String>,
}

impl Default for ModelInventoryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelInventoryBuilder {
    /// Creates a builder with default canonical-ID fallback mappings.
    #[must_use]
    pub fn new() -> Self {
        let mut canonical_id_fallbacks = FxHashMap::default();
        for (pascal, canonical) in DEFAULT_CANONICAL_ID_FALLBACKS {
            canonical_id_fallbacks.insert((*pascal).to_owned(), (*canonical).to_owned());
        }
        Self {
            canonical_id_fallbacks,
        }
    }

    /// Adds or overrides a single canonical-ID fallback mapping.
    #[must_use]
    pub fn with_fallback(mut self, pascal_name: &str, canonical_id: &str) -> Self {
        self.canonical_id_fallbacks
            .insert(pascal_name.to_owned(), canonical_id.to_owned());
        self
    }

    /// Adds or overrides multiple canonical-ID fallback mappings.
    #[must_use]
    pub fn with_fallbacks<I, K, V>(mut self, fallbacks: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (pascal_name, canonical_id) in fallbacks {
            self.canonical_id_fallbacks
                .insert(pascal_name.into(), canonical_id.into());
        }
        self
    }

    /// Builds a deterministic inventory from a [`ModelRegistry`].
    ///
    /// This builder requires both filename conventions and export evidence:
    /// - filename-derived canonical IDs generate link candidates for model files;
    /// - exported symbols must agree with the canonical ID for candidates to be accepted.
    #[must_use]
    pub fn build(&self, registry: &ModelRegistry) -> ModelInventory {
        let mut pending_records: FxHashMap<(ModelSource, String), PendingInventoryRecord> =
            FxHashMap::default();
        let mut fallback_usage_counts: FxHashMap<String, usize> = FxHashMap::default();

        let mut definitions: Vec<_> = registry.iter_all_models().collect();
        definitions.sort_by(|left, right| {
            source_order(left.source)
                .cmp(&source_order(right.source))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| {
                    left.definition_path
                        .as_str()
                        .cmp(right.definition_path.as_str())
                })
        });

        for definition in definitions {
            let path = definition.definition_path.as_path();
            let is_model_file = is_model_path(path);
            let is_interfaces = is_interfaces_file(path);

            let expected_model_file_canonical = if is_model_file {
                canonical_id_from_file_path(self, path, &mut fallback_usage_counts)
            } else {
                None
            };

            let mut sorted_exports: Vec<&str> =
                definition.exports.iter().map(String::as_str).collect();
            sorted_exports.sort_unstable();

            for export_name in sorted_exports {
                let Some((base_pascal, category)) = infer_export_identity(export_name) else {
                    continue;
                };

                if is_interfaces
                    && matches!(category, ModelCategory::Model | ModelCategory::Service)
                {
                    continue;
                }

                let canonical_id =
                    self.canonical_id_from_pascal(&base_pascal, &mut fallback_usage_counts);

                if let Some(expected) = expected_model_file_canonical.as_deref() {
                    if canonical_id != expected {
                        continue;
                    }
                }

                let Some(slot) = slot_for_category(path, category) else {
                    continue;
                };

                let artifact = ModelArtifact {
                    model_name: definition.name.clone(),
                    category,
                    source: definition.source,
                    definition_path: definition.definition_path.clone(),
                    export_name: export_name.to_owned(),
                };

                let key = (definition.source, canonical_id);
                let entry = pending_records.entry(key).or_default();
                entry.push(slot, artifact);
            }
        }

        let mut keyed_records: Vec<_> = pending_records.into_iter().collect();
        keyed_records.sort_by(|(left_key, _), (right_key, _)| {
            source_order(left_key.0)
                .cmp(&source_order(right_key.0))
                .then_with(|| left_key.1.cmp(&right_key.1))
        });

        let mut records = Vec::with_capacity(keyed_records.len());
        let mut ambiguities = Vec::new();

        for ((source, canonical_id), pending) in keyed_records {
            let interface = resolve_slot(
                pending.interface,
                source,
                &canonical_id,
                InventorySlotKind::Interface,
                &mut ambiguities,
            );
            let codegen_interface = resolve_slot(
                pending.codegen_interface,
                source,
                &canonical_id,
                InventorySlotKind::CodegenInterface,
                &mut ambiguities,
            );
            let wrapper = resolve_slot(
                pending.wrapper,
                source,
                &canonical_id,
                InventorySlotKind::Wrapper,
                &mut ambiguities,
            );
            let codegen = resolve_slot(
                pending.codegen,
                source,
                &canonical_id,
                InventorySlotKind::Codegen,
                &mut ambiguities,
            );
            let service_wrapper = resolve_slot(
                pending.service_wrapper,
                source,
                &canonical_id,
                InventorySlotKind::ServiceWrapper,
                &mut ambiguities,
            );
            let service_codegen = resolve_slot(
                pending.service_codegen,
                source,
                &canonical_id,
                InventorySlotKind::ServiceCodegen,
                &mut ambiguities,
            );

            records.push(ModelInventoryRecord {
                source,
                canonical_id,
                interface,
                codegen_interface,
                wrapper,
                codegen,
                service_wrapper,
                service_codegen,
            });
        }

        ambiguities.sort_by(|left, right| {
            source_order(left.source)
                .cmp(&source_order(right.source))
                .then_with(|| left.canonical_id.cmp(&right.canonical_id))
                .then_with(|| slot_order(left.slot).cmp(&slot_order(right.slot)))
        });

        let mut fallback_hits: Vec<_> = fallback_usage_counts
            .into_iter()
            .filter_map(|(pascal_name, count)| {
                self.canonical_id_fallbacks
                    .get(&pascal_name)
                    .map(|canonical_id| CanonicalFallbackHit {
                        pascal_name,
                        canonical_id: canonical_id.clone(),
                        count,
                    })
            })
            .collect();
        fallback_hits.sort_by(|left, right| left.pascal_name.cmp(&right.pascal_name));

        ModelInventory {
            records,
            ambiguities,
            fallback_hits,
        }
    }

    fn canonical_id_from_pascal(
        &self,
        pascal_name: &str,
        fallback_usage_counts: &mut FxHashMap<String, usize>,
    ) -> String {
        if let Some(canonical_id) = self.canonical_id_fallbacks.get(pascal_name) {
            let count = fallback_usage_counts
                .entry(pascal_name.to_owned())
                .or_insert(0);
            *count += 1;
            return canonical_id.clone();
        }

        pascal_to_kebab(pascal_name)
    }
}

/// Builds inventory with the default [`ModelInventoryBuilder`] configuration.
#[must_use]
pub fn build_inventory(registry: &ModelRegistry) -> ModelInventory {
    ModelInventoryBuilder::default().build(registry)
}

fn canonical_id_from_file_path(
    builder: &ModelInventoryBuilder,
    path: &Utf8Path,
    fallback_usage_counts: &mut FxHashMap<String, usize>,
) -> Option<String> {
    let file_stem = path.file_stem()?;
    if file_stem.is_empty() {
        return None;
    }

    let pascal_name = kebab_to_pascal(file_stem);
    if pascal_name.is_empty() {
        return None;
    }

    Some(builder.canonical_id_from_pascal(&pascal_name, fallback_usage_counts))
}

fn resolve_slot(
    mut candidates: Vec<ModelArtifact>,
    source: ModelSource,
    canonical_id: &str,
    slot: InventorySlotKind,
    ambiguities: &mut Vec<InventoryAmbiguity>,
) -> Option<ModelArtifact> {
    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|left, right| {
        left.definition_path
            .as_str()
            .cmp(right.definition_path.as_str())
            .then_with(|| left.export_name.cmp(&right.export_name))
            .then_with(|| category_order(left.category).cmp(&category_order(right.category)))
    });
    candidates.dedup();

    if candidates.len() == 1 {
        return candidates.pop();
    }

    ambiguities.push(InventoryAmbiguity {
        source,
        canonical_id: canonical_id.to_owned(),
        slot,
        candidates,
    });
    None
}

fn infer_export_identity(export_name: &str) -> Option<(String, ModelCategory)> {
    if !looks_like_model_symbol(export_name) {
        return None;
    }

    if let Some(base) = strip_suffix(export_name, "ServiceCodeGen") {
        return Some((base.to_owned(), ModelCategory::ServiceCodeGen));
    }

    if let Some(base) = strip_suffix(export_name, "CodeGenFormArray") {
        return Some((base.to_owned(), ModelCategory::CodeGenFormArray));
    }

    if let Some(base) = strip_suffix(export_name, "CodeGenForApi") {
        return Some((base.to_owned(), ModelCategory::CodeGenForApi));
    }

    if let Some(base) = strip_suffix(export_name, "CodeGenForm") {
        return Some((base.to_owned(), ModelCategory::CodeGenForm));
    }

    if let Some(base) = strip_suffix(export_name, "CodeGen") {
        return Some((base.to_owned(), ModelCategory::CodeGen));
    }

    if let Some(base) = strip_suffix(export_name, "Service") {
        return Some((base.to_owned(), ModelCategory::Service));
    }

    if let Some(base) = strip_suffix(export_name, "Model") {
        return Some((base.to_owned(), ModelCategory::Interface));
    }

    Some((export_name.to_owned(), ModelCategory::Model))
}

fn strip_suffix<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    let base = value.strip_suffix(suffix)?;
    if base.is_empty() {
        None
    } else {
        Some(base)
    }
}

fn looks_like_model_symbol(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    first.is_ascii_uppercase() && chars.all(|ch| ch.is_ascii_alphanumeric())
}

fn slot_for_category(path: &Utf8Path, category: ModelCategory) -> Option<InventorySlotKind> {
    match category {
        ModelCategory::Interface => Some(InventorySlotKind::Interface),
        ModelCategory::CodeGen if is_interfaces_codegen_file(path) => {
            Some(InventorySlotKind::CodegenInterface)
        }
        ModelCategory::CodeGen
        | ModelCategory::CodeGenForApi
        | ModelCategory::CodeGenForm
        | ModelCategory::CodeGenFormArray => Some(InventorySlotKind::Codegen),
        ModelCategory::Model => Some(InventorySlotKind::Wrapper),
        ModelCategory::Service => Some(InventorySlotKind::ServiceWrapper),
        ModelCategory::ServiceCodeGen => Some(InventorySlotKind::ServiceCodegen),
        _ => None,
    }
}

fn is_interfaces_file(path: &Utf8Path) -> bool {
    matches!(
        path.file_name(),
        Some(
            "interfaces.ts" | "interfaces.tsx" | "interfaces.codegen.ts" | "interfaces.codegen.tsx"
        )
    )
}

fn is_interfaces_codegen_file(path: &Utf8Path) -> bool {
    matches!(
        path.file_name(),
        Some("interfaces.codegen.ts" | "interfaces.codegen.tsx")
    )
}

fn is_model_path(path: &Utf8Path) -> bool {
    let raw = path.as_str();
    raw.contains("/models/")
        || raw.contains("\\models\\")
        || raw.ends_with("/models")
        || raw.ends_with("\\models")
}

const fn source_order(source: ModelSource) -> u8 {
    match source {
        ModelSource::SharedLegacy => 0,
        ModelSource::Shared2023 => 1,
        _ => u8::MAX,
    }
}

const fn slot_order(slot: InventorySlotKind) -> u8 {
    match slot {
        InventorySlotKind::Interface => 0,
        InventorySlotKind::CodegenInterface => 1,
        InventorySlotKind::Wrapper => 2,
        InventorySlotKind::Codegen => 3,
        InventorySlotKind::ServiceWrapper => 4,
        InventorySlotKind::ServiceCodegen => 5,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ch_core::ModelDefinition;

    fn definition(
        name: &str,
        source: ModelSource,
        definition_path: &str,
        exports: &[&str],
    ) -> ModelDefinition {
        let mut definition = ModelDefinition::new(name, source, definition_path);
        for export in exports {
            definition.add_export((*export).to_owned());
        }
        definition
    }

    #[test]
    fn test_canonical_fallback_precedence() {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "HTMLReportModel",
            ModelSource::SharedLegacy,
            "shared/interfaces.ts",
            &["HTMLReportModel"],
        ));
        registry.register(definition(
            "HtmlReport",
            ModelSource::SharedLegacy,
            "shared/models/html-report.ts",
            &["HTMLReport"],
        ));

        let inventory = ModelInventoryBuilder::new()
            .with_fallback("HTMLReport", "html-report")
            .build(&registry);

        assert!(inventory
            .find(ModelSource::SharedLegacy, "html-report")
            .is_some());
        assert!(inventory
            .find(ModelSource::SharedLegacy, "h-t-m-l-report")
            .is_none());

        let fallback = inventory
            .fallback_hits()
            .iter()
            .find(|entry| entry.pascal_name == "HTMLReport");
        assert!(fallback.is_some());
        if let Some(entry) = fallback {
            assert_eq!(entry.canonical_id, "html-report");
            assert!(entry.count >= 2);
        }
    }

    #[test]
    fn test_model_chain_linkage() {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "ActiveContractModel",
            ModelSource::SharedLegacy,
            "shared/interfaces.ts",
            &["ActiveContractModel"],
        ));
        registry.register(definition(
            "ActiveContractCodeGen",
            ModelSource::SharedLegacy,
            "shared/interfaces.codegen.ts",
            &["ActiveContractCodeGen"],
        ));
        registry.register(definition(
            "ActiveContract",
            ModelSource::SharedLegacy,
            "shared/models/active-contract.ts",
            &["ActiveContract", "ActiveContractCodeGen"],
        ));

        let inventory = build_inventory(&registry);
        let record = inventory.find(ModelSource::SharedLegacy, "active-contract");
        assert!(record.is_some());
        if let Some(record) = record {
            assert!(record.interface.is_some());
            assert!(record.codegen_interface.is_some());
            assert!(record.wrapper.is_some());
            assert!(record.codegen.is_some());
            assert!(record.has_model_chain());
            assert!(!record.has_service_chain());
        }
        assert!(inventory.ambiguities().is_empty());
    }

    #[test]
    fn test_service_chain_linkage() {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "Customer",
            ModelSource::SharedLegacy,
            "shared/models/customer.ts",
            &["CustomerService", "CustomerServiceCodeGen"],
        ));

        let inventory = build_inventory(&registry);
        let record = inventory.find(ModelSource::SharedLegacy, "customer");
        assert!(record.is_some());
        if let Some(record) = record {
            assert!(record.service_wrapper.is_some());
            assert!(record.service_codegen.is_some());
            assert!(record.has_service_chain());
        }
    }

    #[test]
    fn test_missing_link_candidates_remain_unset() {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "OrderModel",
            ModelSource::SharedLegacy,
            "shared/interfaces.ts",
            &["OrderModel"],
        ));

        let inventory = build_inventory(&registry);
        let record = inventory.find(ModelSource::SharedLegacy, "order");
        assert!(record.is_some());
        if let Some(record) = record {
            assert!(record.interface.is_some());
            assert!(record.wrapper.is_none());
            assert!(record.codegen.is_none());
            assert!(!record.has_model_chain());
        }
    }

    #[test]
    fn test_ambiguous_wrapper_candidates_are_reported() {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "FooFirst",
            ModelSource::SharedLegacy,
            "shared/models/foo.ts",
            &["Foo"],
        ));
        registry.register(definition(
            "FooSecond",
            ModelSource::SharedLegacy,
            "shared/models/codegen/foo.ts",
            &["Foo"],
        ));

        let inventory = build_inventory(&registry);
        let record = inventory.find(ModelSource::SharedLegacy, "foo");
        assert!(record.is_some());
        if let Some(record) = record {
            assert!(record.wrapper.is_none());
        }

        assert_eq!(inventory.ambiguities().len(), 1);
        let ambiguity = &inventory.ambiguities()[0];
        assert_eq!(ambiguity.source, ModelSource::SharedLegacy);
        assert_eq!(ambiguity.canonical_id, "foo");
        assert_eq!(ambiguity.slot, InventorySlotKind::Wrapper);
        assert_eq!(ambiguity.candidates.len(), 2);
    }

    #[test]
    fn test_inventory_records_sorted_deterministically() {
        let mut registry = ModelRegistry::new();
        registry.register(definition(
            "ZooModel",
            ModelSource::Shared2023,
            "shared_2023/interfaces.ts",
            &["ZooModel"],
        ));
        registry.register(definition(
            "AlphaModel",
            ModelSource::SharedLegacy,
            "shared/interfaces.ts",
            &["AlphaModel"],
        ));

        let inventory = build_inventory(&registry);
        let records = inventory.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].source, ModelSource::SharedLegacy);
        assert_eq!(records[0].canonical_id, "alpha");
        assert_eq!(records[1].source, ModelSource::Shared2023);
        assert_eq!(records[1].canonical_id, "zoo");
    }
}
