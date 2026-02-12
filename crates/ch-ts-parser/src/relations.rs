//! Relation extraction from TypeScript source using tree-sitter queries.
//!
//! This module extracts normalized model relation evidence and CST anchors for
//! graph/planner pipelines.

use std::hash::{Hash, Hasher};

use ch_core::{
    AstRelationEvidence, CstAnchor, EdgeKind, FxHashMap, FxHashSet, ImportKind, ModelCategory,
    ModelReference, ModelRegistry, ModelSource, SourceLocation,
};
use rustc_hash::FxHasher;
use smallvec::SmallVec;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, Tree};

use crate::queries::{
    CAPTURE_RELATION_CALL_MEMBER, CAPTURE_RELATION_CALL_OBJECT, CAPTURE_RELATION_CALL_PROPERTY,
    CAPTURE_RELATION_CONSTRUCTS_TARGET, CAPTURE_RELATION_EXTENDS_TARGET,
    CAPTURE_RELATION_IMPLEMENTS_TARGET, CAPTURE_RELATION_INTERFACE_EXTENDS_TARGET,
    CAPTURE_RELATION_SERVICE_PARAM, CAPTURE_RELATION_SERVICE_RETURN,
    CAPTURE_RELATION_TYPE_REF_FIELD, CAPTURE_RELATION_TYPE_REF_PROPERTY,
};
use crate::source::{ModelPathMatcher, detect_model_source_with};

const UNKNOWN_FILE_PATH: &str = "<unknown>";

#[derive(Debug, Clone)]
struct ImportBinding {
    model_name: String,
    source: ModelSource,
    category: ModelCategory,
    import_kind: ImportKind,
    is_type_only: bool,
}

#[derive(Debug, Clone)]
struct ResolvedTarget {
    model: ModelReference,
    import_kind: Option<ImportKind>,
    is_type_only_import: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RelationKey {
    relation: EdgeKind,
    source_name: String,
    source_source: ModelSource,
    target_name: String,
    target_source: ModelSource,
}

impl RelationKey {
    fn new(relation: EdgeKind, source: &ModelReference, target: &ModelReference) -> Self {
        Self {
            relation,
            source_name: source.name.clone(),
            source_source: source.source,
            target_name: target.name.clone(),
            target_source: target.source,
        }
    }
}

struct RelationExtractionContext<'a> {
    source_bytes: &'a [u8],
    bindings: &'a FxHashMap<String, ImportBinding>,
    local_models: &'a FxHashMap<String, ModelCategory>,
    file_source_hint: ModelSource,
}

/// Extracts normalized model relations from a parsed syntax tree.
///
/// # Arguments
///
/// * `tree` - Parsed TypeScript/TSX syntax tree
/// * `source` - Source text used to build the tree
/// * `query` - Compiled relation query
/// * `matcher` - Path matcher used to classify model imports
/// * `registry` - Optional model registry for filtering valid export names
#[must_use]
pub fn extract_model_relations(
    tree: &Tree,
    source: &str,
    query: &Query,
    matcher: &ModelPathMatcher,
    registry: Option<&ModelRegistry>,
) -> SmallVec<[AstRelationEvidence; 16]> {
    let source_bytes = source.as_bytes();
    let root = tree.root_node();

    let bindings = build_import_bindings(root, source_bytes, matcher, registry);
    let local_models = collect_local_model_declarations(root, source_bytes);
    let file_source_hint = infer_file_source_hint(&bindings);
    let context = RelationExtractionContext {
        source_bytes,
        bindings: &bindings,
        local_models: &local_models,
        file_source_hint,
    };

    let mut relations: SmallVec<[AstRelationEvidence; 16]> = SmallVec::new();
    let mut relation_index: FxHashMap<RelationKey, usize> = FxHashMap::default();

    let mut cursor = QueryCursor::new();
    cursor.set_max_start_depth(None);
    let mut matches = cursor.matches(query, root, source_bytes);

    while let Some(query_match) = matches.next() {
        let mut call_object: Option<Node<'_>> = None;
        let mut call_property: Option<Node<'_>> = None;
        let mut call_member: Option<Node<'_>> = None;

        for capture in query_match.captures {
            match capture.index {
                idx if idx == CAPTURE_RELATION_CALL_OBJECT => call_object = Some(capture.node),
                idx if idx == CAPTURE_RELATION_CALL_PROPERTY => call_property = Some(capture.node),
                idx if idx == CAPTURE_RELATION_CALL_MEMBER => call_member = Some(capture.node),
                idx if idx == CAPTURE_RELATION_EXTENDS_TARGET => add_contextual_relation(
                    capture.node,
                    EdgeKind::Extends,
                    false,
                    &context,
                    &mut relations,
                    &mut relation_index,
                ),
                idx if idx == CAPTURE_RELATION_IMPLEMENTS_TARGET => add_contextual_relation(
                    capture.node,
                    EdgeKind::Implements,
                    true,
                    &context,
                    &mut relations,
                    &mut relation_index,
                ),
                idx if idx == CAPTURE_RELATION_INTERFACE_EXTENDS_TARGET => add_contextual_relation(
                    capture.node,
                    EdgeKind::Extends,
                    true,
                    &context,
                    &mut relations,
                    &mut relation_index,
                ),
                idx if idx == CAPTURE_RELATION_TYPE_REF_PROPERTY
                    || idx == CAPTURE_RELATION_TYPE_REF_FIELD =>
                {
                    add_contextual_relation(
                        capture.node,
                        EdgeKind::TypeRef,
                        true,
                        &context,
                        &mut relations,
                        &mut relation_index,
                    );
                }
                idx if idx == CAPTURE_RELATION_SERVICE_PARAM => {
                    let relation = if is_service_context(capture.node, context.source_bytes) {
                        EdgeKind::ServiceParamType
                    } else {
                        EdgeKind::TypeRef
                    };

                    add_contextual_relation(
                        capture.node,
                        relation,
                        true,
                        &context,
                        &mut relations,
                        &mut relation_index,
                    );
                }
                idx if idx == CAPTURE_RELATION_SERVICE_RETURN => {
                    let relation = if is_service_context(capture.node, context.source_bytes) {
                        EdgeKind::ServiceReturnType
                    } else {
                        EdgeKind::TypeRef
                    };

                    add_contextual_relation(
                        capture.node,
                        relation,
                        true,
                        &context,
                        &mut relations,
                        &mut relation_index,
                    );
                }
                idx if idx == CAPTURE_RELATION_CONSTRUCTS_TARGET => add_contextual_relation(
                    capture.node,
                    EdgeKind::Constructs,
                    false,
                    &context,
                    &mut relations,
                    &mut relation_index,
                ),
                _ => {}
            }
        }

        if let (Some(object_node), Some(property_node), Some(member_node)) =
            (call_object, call_property, call_member)
        {
            add_call_relation(
                object_node,
                property_node,
                member_node,
                &context,
                &mut relations,
                &mut relation_index,
            );
        }
    }

    relations
}

fn add_contextual_relation(
    target_node: Node<'_>,
    relation: EdgeKind,
    type_only_context: bool,
    context: &RelationExtractionContext<'_>,
    relations: &mut SmallVec<[AstRelationEvidence; 16]>,
    relation_index: &mut FxHashMap<RelationKey, usize>,
) {
    let Some(source_model) = resolve_source_model(
        target_node,
        context.source_bytes,
        context.local_models,
        context.file_source_hint,
    ) else {
        return;
    };

    let Some(target) = resolve_target_model(
        target_node,
        context.source_bytes,
        context.bindings,
        context.local_models,
        context.file_source_hint,
        false,
    ) else {
        return;
    };

    let is_type_only = type_only_context || target.is_type_only_import;
    let anchor = build_anchor(target_node, context.source_bytes, is_type_only, target.import_kind);
    upsert_relation(relations, relation_index, relation, source_model, target.model, anchor);
}

fn add_call_relation(
    object_node: Node<'_>,
    property_node: Node<'_>,
    member_node: Node<'_>,
    context: &RelationExtractionContext<'_>,
    relations: &mut SmallVec<[AstRelationEvidence; 16]>,
    relation_index: &mut FxHashMap<RelationKey, usize>,
) {
    let Some(source_model) = resolve_source_model(
        member_node,
        context.source_bytes,
        context.local_models,
        context.file_source_hint,
    ) else {
        return;
    };

    let Some(property_name) = node_text(property_node, context.source_bytes) else {
        return;
    };

    if is_factory_method(property_name) {
        let Some(target) = resolve_target_model(
            object_node,
            context.source_bytes,
            context.bindings,
            context.local_models,
            context.file_source_hint,
            false,
        ) else {
            return;
        };

        let anchor = build_anchor(member_node, context.source_bytes, false, target.import_kind);
        upsert_relation(
            relations,
            relation_index,
            EdgeKind::FactoryCall,
            source_model,
            target.model,
            anchor,
        );
        return;
    }

    if is_model_map_object(object_node, context.source_bytes)
        && looks_model_identifier(property_name)
    {
        let Some(target) = resolve_target_name(
            property_name,
            context.bindings,
            context.local_models,
            context.file_source_hint,
            true,
        ) else {
            return;
        };

        let anchor = build_anchor(member_node, context.source_bytes, false, target.import_kind);
        upsert_relation(
            relations,
            relation_index,
            EdgeKind::ModelMapRegistration,
            source_model,
            target.model,
            anchor,
        );
    }
}

fn build_import_bindings(
    root: Node<'_>,
    source_bytes: &[u8],
    matcher: &ModelPathMatcher,
    registry: Option<&ModelRegistry>,
) -> FxHashMap<String, ImportBinding> {
    let mut bindings: FxHashMap<String, ImportBinding> = FxHashMap::default();
    let mut cursor = root.walk();

    for statement in root.named_children(&mut cursor) {
        if statement.kind() != "import_statement" {
            continue;
        }

        let Some(source_node) = statement.child_by_field_name("source") else {
            continue;
        };
        let Some(import_path) = node_text(source_node, source_bytes) else {
            continue;
        };
        let Some(model_source) = detect_model_source_with(import_path, matcher) else {
            continue;
        };

        let is_type_only = statement_is_type_only(statement, source_bytes);
        let Some(clause_node) = child_by_kind(statement, "import_clause") else {
            continue;
        };

        let mut clause_cursor = clause_node.walk();
        for clause_child in clause_node.named_children(&mut clause_cursor) {
            match clause_child.kind() {
                "identifier" => {
                    if let Some(local_name) = node_text(clause_child, source_bytes) {
                        let import_kind =
                            if is_type_only { ImportKind::TypeOnly } else { ImportKind::Default };
                        insert_binding(
                            &mut bindings,
                            local_name,
                            local_name,
                            model_source,
                            import_kind,
                            is_type_only,
                            registry,
                        );
                    }
                }
                "namespace_import" => {
                    let mut namespace_cursor = clause_child.walk();
                    for identifier in clause_child.named_children(&mut namespace_cursor) {
                        if identifier.kind() != "identifier" {
                            continue;
                        }

                        if let Some(local_name) = node_text(identifier, source_bytes) {
                            let import_kind = if is_type_only {
                                ImportKind::TypeOnly
                            } else {
                                ImportKind::Namespace
                            };
                            insert_binding(
                                &mut bindings,
                                local_name,
                                local_name,
                                model_source,
                                import_kind,
                                is_type_only,
                                registry,
                            );
                        }
                    }
                }
                "named_imports" => {
                    let mut named_cursor = clause_child.walk();
                    for import_specifier in clause_child.named_children(&mut named_cursor) {
                        if import_specifier.kind() != "import_specifier" {
                            continue;
                        }

                        let Some(imported_node) = import_specifier.child_by_field_name("name")
                        else {
                            continue;
                        };
                        let Some(imported_name) = node_text(imported_node, source_bytes) else {
                            continue;
                        };
                        let canonical_name = strip_quotes(imported_name);
                        if canonical_name.is_empty() {
                            continue;
                        }

                        let local_name = import_specifier
                            .child_by_field_name("alias")
                            .and_then(|alias| node_text(alias, source_bytes))
                            .unwrap_or(canonical_name);

                        let import_kind =
                            if is_type_only { ImportKind::TypeOnly } else { ImportKind::Named };
                        insert_binding(
                            &mut bindings,
                            local_name,
                            canonical_name,
                            model_source,
                            import_kind,
                            is_type_only,
                            registry,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    bindings
}

fn collect_local_model_declarations(
    root: Node<'_>,
    source_bytes: &[u8],
) -> FxHashMap<String, ModelCategory> {
    let mut models = FxHashMap::default();
    collect_local_model_declarations_recursive(root, source_bytes, &mut models);
    models
}

fn collect_local_model_declarations_recursive(
    node: Node<'_>,
    source_bytes: &[u8],
    models: &mut FxHashMap<String, ModelCategory>,
) {
    match node.kind() {
        "class_declaration" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name_node| node_text(name_node, source_bytes))
            {
                models.insert(name.to_owned(), infer_model_category(name));
            }
        }
        "interface_declaration" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name_node| node_text(name_node, source_bytes))
            {
                models.insert(name.to_owned(), ModelCategory::Interface);
            }
        }
        "type_alias_declaration" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name_node| node_text(name_node, source_bytes))
            {
                models.insert(name.to_owned(), ModelCategory::Interface);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_local_model_declarations_recursive(child, source_bytes, models);
    }
}

fn resolve_source_model(
    node: Node<'_>,
    source_bytes: &[u8],
    local_models: &FxHashMap<String, ModelCategory>,
    file_source_hint: ModelSource,
) -> Option<ModelReference> {
    let mut current = Some(node);

    while let Some(cursor_node) = current {
        let category = match cursor_node.kind() {
            "class_declaration" | "interface_declaration" | "type_alias_declaration" => cursor_node
                .child_by_field_name("name")
                .and_then(|name_node| node_text(name_node, source_bytes))
                .and_then(|name| local_models.get(name).copied().map(|category| (name, category))),
            _ => None,
        };

        if let Some((name, model_category)) = category {
            return Some(ModelReference::new(name, model_category, file_source_hint));
        }

        current = cursor_node.parent();
    }

    None
}

fn resolve_target_model(
    target_node: Node<'_>,
    source_bytes: &[u8],
    bindings: &FxHashMap<String, ImportBinding>,
    local_models: &FxHashMap<String, ModelCategory>,
    file_source_hint: ModelSource,
    allow_unbound_fallback: bool,
) -> Option<ResolvedTarget> {
    let mut candidate_names: SmallVec<[String; 8]> = SmallVec::new();
    collect_candidate_names(target_node, source_bytes, &mut candidate_names);

    let mut seen: FxHashSet<String> = FxHashSet::default();
    for candidate in candidate_names {
        if !seen.insert(candidate.clone()) || is_ignored_type_name(&candidate) {
            continue;
        }

        if let Some(resolved) = resolve_target_name(
            &candidate,
            bindings,
            local_models,
            file_source_hint,
            allow_unbound_fallback,
        ) {
            return Some(resolved);
        }
    }

    None
}

fn resolve_target_name(
    candidate: &str,
    bindings: &FxHashMap<String, ImportBinding>,
    local_models: &FxHashMap<String, ModelCategory>,
    file_source_hint: ModelSource,
    allow_unbound_fallback: bool,
) -> Option<ResolvedTarget> {
    if let Some(binding) = bindings.get(candidate) {
        return Some(ResolvedTarget {
            model: ModelReference::new(
                binding.model_name.clone(),
                binding.category,
                binding.source,
            ),
            import_kind: Some(binding.import_kind),
            is_type_only_import: binding.is_type_only,
        });
    }

    if let Some(category) = local_models.get(candidate) {
        return Some(ResolvedTarget {
            model: ModelReference::new(candidate, *category, file_source_hint),
            import_kind: None,
            is_type_only_import: false,
        });
    }

    if allow_unbound_fallback
        && looks_model_identifier(candidate)
        && !is_ignored_type_name(candidate)
    {
        return Some(ResolvedTarget {
            model: ModelReference::new(
                candidate,
                infer_model_category(candidate),
                file_source_hint,
            ),
            import_kind: None,
            is_type_only_import: false,
        });
    }

    None
}

fn collect_candidate_names(
    node: Node<'_>,
    source_bytes: &[u8],
    candidates: &mut SmallVec<[String; 8]>,
) {
    match node.kind() {
        "identifier" | "type_identifier" | "property_identifier" => {
            if let Some(name) = node_text(node, source_bytes) {
                candidates.push(name.to_owned());
            }
            return;
        }
        "nested_type_identifier" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                collect_candidate_names(name_node, source_bytes, candidates);
            }
            return;
        }
        "generic_type" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                collect_candidate_names(name_node, source_bytes, candidates);
            }
            if let Some(type_arguments) = node.child_by_field_name("type_arguments") {
                collect_candidate_names(type_arguments, source_bytes, candidates);
            }
            return;
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_candidate_names(child, source_bytes, candidates);
    }
}

fn infer_file_source_hint(bindings: &FxHashMap<String, ImportBinding>) -> ModelSource {
    let mut has_legacy = false;
    let mut has_modern = false;

    for binding in bindings.values() {
        match binding.source {
            ModelSource::SharedLegacy => has_legacy = true,
            ModelSource::Shared2023 => has_modern = true,
            _ => {}
        }
    }

    match (has_legacy, has_modern) {
        (false, true) => ModelSource::Shared2023,
        (true | false, false) | (true, true) => ModelSource::SharedLegacy,
    }
}

fn upsert_relation(
    relations: &mut SmallVec<[AstRelationEvidence; 16]>,
    relation_index: &mut FxHashMap<RelationKey, usize>,
    relation: EdgeKind,
    source_model: ModelReference,
    target_model: ModelReference,
    anchor: CstAnchor,
) {
    let key = RelationKey::new(relation, &source_model, &target_model);

    if let Some(existing_index) = relation_index.get(&key).copied() {
        relations[existing_index].anchors.push(anchor);
        return;
    }

    let mut evidence = AstRelationEvidence::new(relation, source_model, target_model);
    evidence.add_anchor(anchor);
    let index = relations.len();
    relations.push(evidence);
    relation_index.insert(key, index);
}

fn build_anchor(
    node: Node<'_>,
    source_bytes: &[u8],
    is_type_only: bool,
    import_kind: Option<ImportKind>,
) -> CstAnchor {
    let start = node.start_position();
    let end = node.end_position();

    let mut anchor = CstAnchor::new(
        UNKNOWN_FILE_PATH,
        usize_to_u32(node.start_byte()),
        usize_to_u32(node.end_byte()),
        SourceLocation::new(
            usize_to_u32(start.row).saturating_add(1),
            usize_to_u32(start.column),
            usize_to_u32(node.start_byte()),
        ),
        SourceLocation::new(
            usize_to_u32(end.row).saturating_add(1),
            usize_to_u32(end.column),
            usize_to_u32(node.end_byte()),
        ),
        node.kind(),
    );

    anchor.field_name = field_name_for_node(node).map(ToOwned::to_owned);
    anchor.import_kind = import_kind;
    anchor.is_type_only = is_type_only;
    anchor.snippet_hash = node_text(node, source_bytes).map_or(0, hash_snippet);

    anchor
}

fn field_name_for_node(node: Node<'_>) -> Option<&'static str> {
    let parent = node.parent()?;
    let mut cursor = parent.walk();

    for (index, child) in parent.children(&mut cursor).enumerate() {
        if child == node {
            let child_index = u32::try_from(index).ok()?;
            return parent.field_name_for_child(child_index);
        }
    }

    None
}

fn child_by_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| child.kind() == kind)
}

fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let start = node.start_byte();
    let end = node.end_byte();
    std::str::from_utf8(source.get(start..end)?).ok()
}

fn statement_is_type_only(statement: Node<'_>, source_bytes: &[u8]) -> bool {
    node_text(statement, source_bytes)
        .is_some_and(|text| text.trim_start().starts_with("import type"))
}

fn insert_binding(
    bindings: &mut FxHashMap<String, ImportBinding>,
    local_name: &str,
    canonical_name: &str,
    source: ModelSource,
    import_kind: ImportKind,
    is_type_only: bool,
    registry: Option<&ModelRegistry>,
) {
    if let Some(model_registry) = registry {
        if !model_registry.is_export_from(canonical_name, source) {
            return;
        }
    }

    bindings.insert(
        local_name.to_owned(),
        ImportBinding {
            model_name: canonical_name.to_owned(),
            source,
            category: infer_model_category(canonical_name),
            import_kind,
            is_type_only,
        },
    );
}

fn infer_model_category(name: &str) -> ModelCategory {
    if name.ends_with("CodeGenFormArray") {
        return ModelCategory::CodeGenFormArray;
    }
    if name.ends_with("CodeGenForApi") {
        return ModelCategory::CodeGenForApi;
    }
    if name.ends_with("CodeGenForm") {
        return ModelCategory::CodeGenForm;
    }
    if name.ends_with("CodeGen") {
        return ModelCategory::CodeGen;
    }
    if name.ends_with("Model") {
        return ModelCategory::Interface;
    }

    ModelCategory::Model
}

fn is_service_context(node: Node<'_>, source_bytes: &[u8]) -> bool {
    let mut current = Some(node);

    while let Some(cursor_node) = current {
        match cursor_node.kind() {
            "class_declaration" | "function_declaration" => {
                if let Some(name) = cursor_node
                    .child_by_field_name("name")
                    .and_then(|name_node| node_text(name_node, source_bytes))
                {
                    return name.contains("Service");
                }
            }
            _ => {}
        }
        current = cursor_node.parent();
    }

    false
}

fn is_factory_method(method_name: &str) -> bool {
    matches!(
        method_name,
        "toApi"
            | "toFormGroup"
            | "toFormArray"
            | "toModel"
            | "fromApi"
            | "fromFormGroup"
            | "fromFormArray"
    )
}

fn is_model_map_object(node: Node<'_>, source_bytes: &[u8]) -> bool {
    node_text(node, source_bytes).is_some_and(|text| {
        let trimmed = text.trim();
        trimmed == "modelMap" || trimmed.ends_with(".modelMap")
    })
}

fn looks_model_identifier(name: &str) -> bool {
    name.chars().next().is_some_and(|ch| ch.is_ascii_uppercase())
}

fn is_ignored_type_name(name: &str) -> bool {
    matches!(
        name,
        "Array"
            | "ReadonlyArray"
            | "Promise"
            | "Map"
            | "Set"
            | "Record"
            | "Partial"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Date"
            | "String"
            | "Number"
            | "Boolean"
            | "Object"
            | "Function"
            | "unknown"
            | "any"
            | "never"
            | "void"
            | "undefined"
            | "null"
            | "this"
    )
}

fn strip_quotes(s: &str) -> &str {
    s.trim_matches(|c| c == '"' || c == '\'')
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn hash_snippet(snippet: &str) -> u64 {
    let mut hasher = FxHasher::default();
    snippet.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::{Language, Parser, Query};

    use crate::queries::{RELATION_QUERY, get_tsx_relation_query, get_typescript_relation_query};

    fn create_parser() -> Parser {
        let mut parser = Parser::new();
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        parser.set_language(&language).expect("Failed to set TypeScript language");
        parser
    }

    fn create_tsx_parser() -> Parser {
        let mut parser = Parser::new();
        let language: Language = tree_sitter_typescript::LANGUAGE_TSX.into();
        parser.set_language(&language).expect("Failed to set TSX language");
        parser
    }

    fn create_query() -> Query {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        Query::new(&language, RELATION_QUERY).expect("Relation query should compile")
    }

    #[test]
    fn test_extract_interface_property_type_ref() {
        let source = r#"
import type { OrderModel } from '../shared/models/order-model';

export interface CustomerModel {
    order: OrderModel;
}
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse should succeed");
        let query = create_query();

        let relations =
            extract_model_relations(&tree, source, &query, &ModelPathMatcher::default(), None);

        assert_eq!(relations.len(), 1);
        let relation = &relations[0];
        assert_eq!(relation.relation, EdgeKind::TypeRef);
        assert_eq!(relation.source.name, "CustomerModel");
        assert_eq!(relation.target.name, "OrderModel");
        assert!(relation.anchors[0].is_type_only);
        assert_eq!(relation.anchors[0].import_kind, Some(ImportKind::TypeOnly));
    }

    #[test]
    fn test_extract_class_extends_and_implements() {
        let source = r#"
import { BaseModel, Trackable } from '../shared_2023/models/base-model';

export class OrderModel extends BaseModel implements Trackable {
}
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse should succeed");
        let query = create_query();

        let relations =
            extract_model_relations(&tree, source, &query, &ModelPathMatcher::default(), None);

        assert_eq!(relations.len(), 2);
        assert!(relations.iter().any(|relation| relation.relation == EdgeKind::Extends
            && relation.target.name == "BaseModel"));
        assert!(relations.iter().any(|relation| relation.relation == EdgeKind::Implements
            && relation.target.name == "Trackable"));
    }

    #[test]
    fn test_extract_constructs_and_factory_calls() {
        let source = r#"
import { OrderModel } from '../shared/models/order-model';

class OrderService {
    create(): OrderModel {
        return new OrderModel();
    }

    toApi(): unknown {
        return OrderModel.toApi();
    }

    toForm(): unknown {
        return OrderModel.toFormGroup();
    }
}
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse should succeed");
        let query = create_query();

        let relations =
            extract_model_relations(&tree, source, &query, &ModelPathMatcher::default(), None);

        assert!(relations.iter().any(|relation| relation.relation == EdgeKind::Constructs
            && relation.target.name == "OrderModel"));
        let factory_count =
            relations.iter().filter(|relation| relation.relation == EdgeKind::FactoryCall).count();
        assert_eq!(factory_count, 2);
    }

    #[test]
    fn test_extract_model_map_registration() {
        let source = r#"
class OrderService {
    buildMap() {
        return modelMap.OrderModel();
    }
}
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse should succeed");
        let query = create_query();

        let relations =
            extract_model_relations(&tree, source, &query, &ModelPathMatcher::default(), None);

        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].relation, EdgeKind::ModelMapRegistration);
        assert_eq!(relations[0].target.name, "OrderModel");
    }

    #[test]
    fn test_extract_alias_and_type_only_imports() {
        let source = r#"
import type { OrderModel as LegacyOrderModel } from '../shared/models/order-model';

interface MapperModel {
    target: LegacyOrderModel;
}
"#;
        let mut parser = create_parser();
        let tree = parser.parse(source, None).expect("Parse should succeed");
        let query = create_query();

        let relations =
            extract_model_relations(&tree, source, &query, &ModelPathMatcher::default(), None);

        assert_eq!(relations.len(), 1);
        let relation = &relations[0];
        assert_eq!(relation.target.name, "OrderModel");
        assert!(relation.anchors[0].is_type_only);
    }

    #[test]
    fn test_extract_tsx_relations() {
        let source = r#"
import { UiModel } from '../shared_2023/models/ui-model';

class DashboardModel {
    create() {
        return <div>{new UiModel().toApi()}</div>;
    }
}
"#;
        let mut parser = create_tsx_parser();
        let tree = parser.parse(source, None).expect("Parse should succeed");
        let query = get_tsx_relation_query().expect("TSX relation query should compile");

        let relations =
            extract_model_relations(&tree, source, query, &ModelPathMatcher::default(), None);

        assert!(relations.iter().any(|relation| relation.relation == EdgeKind::Constructs
            && relation.target.name == "UiModel"));
        assert!(relations.iter().any(|relation| relation.relation == EdgeKind::FactoryCall
            && relation.target.name == "UiModel"));
    }

    #[test]
    fn test_relation_queries_cached() {
        let ts_query = get_typescript_relation_query().expect("TS relation query should compile");
        let tsx_query = get_tsx_relation_query().expect("TSX relation query should compile");

        assert!(ts_query.pattern_count() > 0);
        assert!(tsx_query.pattern_count() > 0);
    }
}
