# Phase 4 Result Exports (Golden 14 Surface) — 2026-09-21

Related: [[Phase_4_Desktop_Ordination_Tauri_Adapter_2026-09-20]], [[Phase_4_Explore_Views_Golden_12_2026-09-19]], [[Phase_4_UMAP_Naive_Port_Golden_07_2026-09-20]]

## Summary

Recovered the interrupted exports slice found uncommitted in the working tree
and landed it: Section 7.3 result exports — measured chemical data, computed
transformed results, and PCA scores as ephemeral CSV — wired end-to-end
through contracts, application, HTTP, and the Tauri desktop adapter.
Commit `e253b87`.

## What was built

- `crates/contracts`: `ExportMeasuredDataRequest` (`raw_text` default false),
  `ExportTransformedRequest` (inline definition), `ExportPcaScoresRequest`
  (columns, `scale`, optional transformation), `ExportResult`
  (file_name/media_type/content), with serde round-trip tests.
- `crates/application/src/exports.rs`: `ExportService` +
  `ensure_csv_extension` (legacy `saveexportTab.R` name rule: keep as typed
  if it contains a dot, else `.csv`; XLSX/TSV deferred per Section 7.1).
  Formula-injection guard (`= + - @` prefixed with `'` unless `raw_text` or a
  finite numeric). Numbers via the shared `r_format_double`. The hidden
  `analytical_uuid` never appears in exported content (asserted in tests).
  PCA export follows the Section 3.2 correction of the legacy `rvals$pcaData`
  bug: it exports the computed `pcadf`-equivalent score frame. Membership
  export is Phase 6 (needs the membership engine).
- `crates/api`: `POST /api/v1/exports/{measured-data,transformed,pca-scores}`
  + round-trip/422 test.
- `crates/desktop` + `apps/desktop/src-tauri`: `DesktopExports` and three
  `export_*` commands, registered in `generate_handler!`.

## Verification

- `cargo fmt --check`, `tools/cargo-lint.sh` (clippy -D warnings),
  `cargo test --workspace` (141 tests, 43 suites), `cargo tauri build
  --no-bundle` all green.
- Reviewer subagent: STATUS APPROVED. Nits (deferred): `numeric_cell(Some(...))`
  could replace the inline PCA cell formatting; `export_pca_scores` reads the
  group file twice (once inside `OrdinationService::pca`); injection guard
  does not cover a leading tab (cosmetic; `csv_field` quotes CR).
- Environment note: rustup/toolchain 1.98.1 (rustfmt/clippy) and tauri-cli had
  to be reinstalled (environment reset); `cargo tauri build` rewrites
  `[build-dependencies] tauri-build` to `features = []` — revert that diff
  before committing.

## Status

Phase 4 backend remaining: preferences; the React client surface
(packages/client) is still pending across all phases. Golden 14 (measured-data
export round trip) parity baselines now have a Rust-side consumer.
