# Phase 3 Transform Definitions 2026-09-17

Up: [[../IMPLEMENTATION]]

## Outcome

The Phase 3 named-transformation surface (Section 8.2) now exists end to end
through both transport adapters, reusing the golden-verified transform
primitives from `crates/analysis` (zScore, log/log10, ratios — goldens #2–4
already passing in `tests/parity/tests/golden_transforms.rs`):

- **`crates/contracts`** — `TransformMethod` (`none`/`log`/`log10`/`zScore`,
  wire names matching the legacy R values exactly), `ImputationMethod`
  (`none`/`pmm`/`midastouch`/`rf`), `RatioSpecDto` (optional output name
  defaulting to the deterministic `numerator_denominator` form),
  `RatioMode` (`append`/`only`), `TransformationDefinition`,
  `TransformationSummary`, save/list/apply request-response DTOs, and
  `AppliedTransformation` (row-major nullable rows + `non_finite_to_zero`
  warning count).
- **`crates/application/src/transforms.rs`** — `TransformService`: JSON
  persistence under `.archaeodash/transformations/<name>.json` (sanitized and
  capped at 32 chars per the legacy `transform_table_name_max_len`, atomic
  tmp+rename writes, creation timestamp preserved on upsert — replacing the
  legacy unit-separator/pipe DB encoding per §8.2); `save` (upsert +
  `replaced` flag), `list` (name-sorted), `load`/`delete` (404); validation
  covering empty names, missing elemental columns, the §8.3 imputation parity
  gate (`seed_required` then `imputation_not_gated`), ratio identity
  (`num==den`), and duplicate output names; `batch_ratio_specs`
  (one-to-one with length-mismatch check, Cartesian, self-pair exclusion,
  `_2`/`_3` dedup); `apply` — group file → NaN-as-NA column matrix → base
  transform → ratios → optional `only` mode, fully ephemeral (Section 5
  storage invariant: calculated values never written to group files).
- **`crates/api`** — `TransformService` in `AppState`;
  `domain_error_response` mapping (Validation/InvalidIdentity → 422,
  NotFound → 404, Internal → 500); routes `POST/GET /api/v1/transformations`,
  `GET/DELETE /api/v1/transformations/{name}`,
  `POST /api/v1/transformations/ratios/batch`,
  `POST /api/v1/transformations/apply`.
- **`crates/desktop` + Tauri shell** — `DesktopTransforms` with six commands
  (`save_transformation`, `list_transformations`, `load_transformation`,
  `delete_transformation`, `batch_ratio_specs`, `apply_transformation`),
  all registered in `generate_handler![]`.

## Key discoveries

- Serde default enum naming doesn't match legacy R method strings:
  `Log10` serializes as `"Log10"` and `rename_all = "snake_case"` yields
  `midas_touch` — both need explicit `#[serde(rename)]` to hit the legacy
  `log10`/`midastouch` wire names.
- `ColumnMatrix` is column-major while `AppliedTransformation.rows` is
  row-major; the transpose must capture `n_rows()` before moving `names`
  (partial-move borrow error otherwise).
- Cartesian batch of `[as,fe]×[fe,as]` has no duplicate pairs — dedup
  suffixes only appear for repeated identical pairs.
- `apply_ratios` operates on raw (untransformed) matrices in golden #4, so
  `apply` chains base transform first, then ratios, matching the oracle.

## Verification

`cargo fmt` clean; `cargo clippy --workspace -- -D warnings` clean; full
workspace `cargo test` green: **103 passed, 0 failed** (was 91). New tests:
5 application (round trip, validation codes, batch naming, ephemeral apply
with byte-identical group file, only-mode/missing-column), 1 contracts
round-trip with legacy wire-name assertions, 1 HTTP integration
(save→list→load→apply→batch→422→404→delete→404), 1 desktop round trip.

## Remaining in Phase 3

Imputation methods (`pmm`, `midastouch`, `rf`) stay behind the
`imputation_not_gated` rejection until the R oracle exports MICE package
version, defaults, RNG kind, and seeded goldens (§8.3 prerequisite).
Selection/group controls, metadata refresh, permutation configuration, and
job progress/cancel are the other Phase 3 items (Section 16), followed by
Phases 4–6 (ordination/Explore, visualize/assign, cluster/membership/
Euclidean) and Phase 7 (hosted auth).

## Related

- [[Phase_2_Source_File_Catalog_2026-09-17]]
- [[Phase_2_Group_Delete_Route_2026-09-17]]
