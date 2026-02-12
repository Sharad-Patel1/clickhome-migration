# Graph planner fixtures

This directory contains trimmed fixtures used for end-to-end graph + planning tests.

## ng15-mini

Source: `/Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app`

Retained files (trimmed for determinism):
- `app/shared/interfaces.ts` — legacy interface export used for inventory coverage.
- `app/shared/helpers/bind-model.ts` — helper referenced by legacy models.
- `app/shared/models/collection.ts` — legacy model baseline.
- `app/shared/models/collection-for-api.ts` — legacy codegen wrapper shape.
- `app/shared/models/doc-client-shares.ts` — legacy model baseline.
- `app/shared/models/doc-construction-client-shares.ts` — legacy model baseline.
- `app/shared/models/meta-data.ts` — legacy model with helper dependency.
- `app/shared_2023/interfaces.codegen.ts` — modern interface/codegen coverage.
- `app/shared_2023/models/collection.ts` — modern model baseline.
- `app/shared_2023/models/file.ts` — modern model with type reference.
- `app/shared_2023/models/meta-data.ts` — modern model baseline.
- `app/features/collection-consumer.ts` — trimmed consumer ensuring mixed legacy/modern imports.

Notes:
- All files are trimmed to the minimum needed to preserve model export names and
  type relationships.
- Comments in each file record the origin path for traceability.

### Refresh workflow

1. Copy the relevant source files from the ng-15 repository into the same
   relative paths under `ng15-mini/`.
2. Trim file contents to only the exported types/classes and minimal imports
   required to compile.
3. Ensure `app/features/collection-consumer.ts` still exercises both
   `shared/` and `shared_2023/` imports.
4. Update the contract summary snapshot:
   - Run `cargo test -p ch-cli test_ng15_fixture_contract_summary_matches_golden_snapshot`
   - Replace `test-fixtures/graph-planner/golden/ng15-mini/contract-summary.json`
     with the updated summary output if the schema changed.
