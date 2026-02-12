# Graph planner test matrix

This matrix maps acceptance coverage for the phase-1 graph planning milestone to
concrete automated tests and fixtures.

| Coverage area | Tests | Fixture/notes |
| --- | --- | --- |
| Parser relation extraction | `crates/ch-ts-parser/src/relations.rs` (relation query unit tests) | Uses inline source snippets to validate extends/implements/type refs, constructors, factories, and map registrations. |
| Import extraction + registry matching | `crates/ch-ts-parser/src/import.rs`, `crates/ch-scanner/src/analyzer.rs` | Validates shared/shared_2023 import classification and model ref generation. |
| Inventory and mapping shape matching | `crates/ch-graph/src/comparator.rs` (`test_compare_exact_match_with_reasons`, `test_compare_renamed_mapping_with_usage_fallback`, `test_compare_no_match_and_low_confidence_thresholds`) | Exercises confidence scoring, fallbacks, and no-match behavior. |
| Residual legacy usage detection | `crates/ch-graph/src/comparator.rs::test_compare_emits_residual_legacy_usage` | Confirms residual reporting in diff output. |
| Graph build determinism | `crates/ch-graph/src/builder.rs::test_build_is_deterministic_across_input_ordering` | Ensures node/edge ordering is stable. |
| SCC/condensation + plan ordering | `crates/ch-graph/src/planner.rs::test_plan_orders_components_and_has_valid_prerequisites` | Verifies component condensation, topo ordering, and prerequisite consistency. |
| Plan stability across file order | `crates/ch-graph/src/planner.rs::test_plan_is_deterministic_across_file_order` | Confirms deterministic plan output. |
| Artifact export determinism | `crates/ch-graph/src/export.rs::test_export_outputs_are_deterministic_excluding_timestamp` | Graph/plan JSON + DOT/MD are stable after timestamp normalization. |
| CLI artifact contract snapshot | `crates/ch-cli/src/main.rs::test_ng15_fixture_contract_summary_matches_golden_snapshot` | Golden contract summary over ng15-mini fixture output. |
| CLI reproducibility loop | `crates/ch-cli/src/main.rs::test_ng15_fixture_artifacts_are_reproducible_across_repeated_runs` | Runs full pipeline 3x and compares normalized artifacts. |
| Real-world mixed legacy/modern fixture | `test-fixtures/graph-planner/ng15-mini` | Trimmed ng-15 slice with cross-import consumer. |
