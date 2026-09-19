# Phase 4 Explore Views Golden 12 — 2026-09-19

Related: [[Phase_4_Ordination_PCA_LDA_Research_2026-09-17]], [[Phase_4_LDA_HTTP_Adapter_And_Group_Gate_Test_2026-09-18]]

## Summary

Continued the interrupted Phase 4 session: finished `crates/analysis/src/explore.rs`
(procedure 12, class E), added the golden-12 parity test, and wired Explore
end-to-end through contracts, application, HTTP, and the Tauri desktop adapter.

## Key findings

- The draft `explore.rs` recovered from the dead session had a corrupted test
  literal, a transposed `compositional_profile` (element-major instead of
  `pivot_longer` row-major: every rowid lists all elements before the next
  rowid), and a missing-profile ordering bug: legacy `plot_missing` orders the
  feature factor by `order(-rank(num_missing))` - descending missing count,
  stable ties in column order.
- Golden 12 (`fixtures/golden/12_explore_views.json`) captures only three
  views: `missing_profile` (8 rows, capitalized `Band` key), `histogram`
  (first base-chem column, `breaks = 30` -> 26 breaks / 25 counts), and
  `compositional_profile` (2456 = 307 x 8 rows). No crosstab was captured.
- `pretty(c(0, 12.0236), 30, min.n = 1)` on R 4.6.1 gives 0 to 12.5 by 0.5 -
  verified with `Rscript` directly (R-4.6.1 is vendored in the repo root).
- Legacy `compute_crosstab_summary`: `count` groups by both columns as text;
  `mean`/`median`/`sd` coerce the value column via
  `as.numeric(as.character(...))`, error when no value converts, group by the
  group column only, and round to 2 decimals.

## Implementation

- `crates/analysis`: `missing_profile`, `r_pretty`, `histogram`,
  `crosstab_count`, `crosstab_value_summary`, `compositional_profile`, each
  with unit tests including R 4.6.1 oracle vectors for `pretty`.
- `tests/parity/tests/golden_12_explore.rs`: pins all three captured views
  (`assert_exact` for breaks/counts/rowids, 5.1e-5 for jsonlite-rounded
  values).
- `crates/contracts`: `Explore*Request/Response` DTOs with a
  `kind`-tagged `CrosstabRows` enum (count vs summary shape).
- `crates/application`: `ExploreService` reusing the ordination input-matrix
  path (ephemeral, never persists); crosstab count renders elemental values
  as text via `r_format_double`.
- `crates/api`: `POST /api/v1/explore/missing-profile`, `/histogram`,
  `/crosstab`, `/compositional-profile` + round-trip/422 test.
- `crates/desktop` + `apps/desktop/src-tauri`: `DesktopExplore` and four
  `explore_*` commands (PCA/LDA still have no Tauri commands - next slice).
- Clippy (new toolchain) flagged `sort_by` -> `sort_by_key`, a
  `double_comparison`, two `partial_cmp().expect()` -> `total_cmp`, and the
  pre-existing `lda.rs` `binary_search().expect()` (now `unreachable!`
  guarded).

## Status

Workspace tests 42 suites green, `tools/cargo-lint.sh` green, Tauri shell
compiles. Remaining in Phase 4: UMAP (`umap_rs` spike, golden 7), Explore UI
preferences/exports, descriptive-edit table (Phase 4 scope per
[[../IMPLEMENTATION]]).

## Related

- [[Phase_4_Ordination_PCA_LDA_Research_2026-09-17]]
- [[Phase_4_LDA_HTTP_Adapter_And_Group_Gate_Test_2026-09-18]]
