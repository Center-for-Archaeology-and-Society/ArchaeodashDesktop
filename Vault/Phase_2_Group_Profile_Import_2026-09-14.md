# Phase 2 Group Profile and Import 2026-09-14

Up: [[../IMPLEMENTATION]]

## Outcome

Phase 2 partial delivery (import + transforms + storage core), all verified against the R oracle goldens:

- **`crates/data-io`** (green, clippy-clean):
  - `rnum.rs` — R-compatible numerics: `parse_r_numeric` (`NA`/`NaN` are NA-class, `Inf` parses, `Infinity` does not), `r_format_double` (15 significant digits, trailing-zero trim, R scientific form `1e-05`/`1e+15`), `r_round` with FMA two-product tie correction reproducing R's long-double scaling (`round(2.675,2)=2.67`, `round(0.0015,3)=0.002`), `compensated_sum` (Neumaier).
  - `clean_names.rs` — full `janitor::clean_names(case="none")` port verified character-for-character against the oracle env (special-char map, leading strip, collapse-to-dot, `make.names` X-prefix/reserved-word, split-on-dot join, snapshot-based dedupe `as/as_2/as_3`).
  - `loader.rs` — `TextFrame`, `data_loader` (clean names, drop incoming `rowid`, prepend 1-based rowid), `default_chem_columns`/`default_id_column`, `guess_numeric_columns_fast` (1,500-row/95% rule, NaN text counts as parse failure matching `is.na`), `group_partitions` with blank-group rejection.
  - `group_profile.rs` — Group Parquet profile v1: `archaeodash.profile.v1` written both as explicit Parquet footer KV (via `WriterProperties::set_key_value_metadata` with `parquet::file::metadata::KeyValue`) and Arrow schema metadata; read falls back across both. 16-byte `analytical_uuid` identity column, schema-role validation, unique-UUID check, measured-element SHA-256 checksum over `(uuid, col, value|null)` tuples. `write_group_file` preserves identities across revisions (`existing_uuids`), `read_group_uuids`, `scan_project` (manifest-free discovery, `ReadyToAdd`/`ForeignParquet`/`NotParquet`), `validate_group_file`, derived-column storage invariant.
- **`crates/analysis`**: `z_score` (prop.table×100 → scale with na.rm=TRUE mean/sd → round-3), `log_transform` (non-finite→zero + count), `apply_ratios` (null on zero/NA denominator), `ColumnMatrix`.
- **`tests/parity`**: goldens #1–#4 pass exactly (class E). Harness helpers `assert_exact`/`assert_within_serialization` (jsonlite 4-decimal serialization tolerance).

## Key discoveries

- arrow-rs 59 embeds Arrow schema metadata inside the base64 `ARROW:schema` footer key; standalone KV needs explicit `WriterProperties`. Diagnosed with a round-trip probe test (since removed).
- R `scale()` uses `na.rm=TRUE` for both mean and sd; `zScore` values differ substantially from naive scaling (`-1.081, 0.188, 0.893` for the 1:6 probe matrix, not `-1, 0, 1`).
- jsonlite goldens serialize at 4 decimals; round-3 outputs compare exactly, raw columns need ±5.1e-5.
- R `round()` ties need the FMA residual trick because naive f64 scaling puts `2.675*100` exactly on the tie.

## Verification

`cargo fmt`/`clippy -D warnings` clean; full workspace tests green; `pnpm -r build`/`test` green. Golden parity #1–#4 pass against `fixtures/golden/*.json` on the real INAA fixture (307 rows, 5 CORE groups).

## Remaining in Phase 2

Golden #14 (CSV export round-trip), `file-store-fs` atomic write-temp-rename-fsync, transaction journal, append/merge operations, HTTP/Tauri exposure of import. Then Phases 3–6 (goldens #5–#13: seeded imputation, PCA/LDA via faer with sign alignment, UMAP, clustering, membership, Euclidean, Explore, multiplot) and Phase 7 (hosted auth).

## Related

- [[Phase_1_Monorepo_Skeleton_2026-09-14]]
- [[R_Oracle_Baseline_Capture_2026-09-14]]
