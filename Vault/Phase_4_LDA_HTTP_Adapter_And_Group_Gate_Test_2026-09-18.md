# Phase 4 LDA HTTP Adapter And Group Gate Test — 2026-09-18

Related: [[Phase_4_Ordination_PCA_LDA_Research_2026-09-17]], [[Phase_3_Transform_Definitions_2026-09-17]]

## Summary

Implemented the `POST /api/v1/ordination/lda` HTTP adapter in `crates/api/src/lib.rs` over
`OrdinationService::lda` (see [[Phase_4_Ordination_PCA_LDA_Research_2026-09-17]]), plus the
`ordination_lda_round_trip_and_group_gate` integration test.

## Key findings

- `GroupService::merge_groups` writes the merged output **to the first source's path**
  (`plan_merge(&sources, 0, ...)`) and archives/deletes `sources[1..]`. A test that merges
  N groups and then re-reads `commit.groups[0].path` is silently reading the *merged* file,
  not the original single-group file.
- The legacy LDA gate (`validate_lda_groups`, min 3) fires only when the file's group column
  has exactly 2 distinct levels; a single-level file fails the earlier
  "grouping factor must have at least 2 levels" check instead.

## Test shape that works

1. Import a 3-group CSV (`Site` = A/B/C) via `/api/v1/imports/commit`.
2. Merge its three group files; the merged output (`merge.outputs[0]`) spans 3 levels.
3. LDA round-trip on the merged path: 200, 3 levels, `LD1`/`LD2`, one score per row.
4. For the 422 gate: import the 2-group fixture (`commit_fixture`, Baca/Hooper), merge its
   two groups into one file, then POST LDA — expect 422 with
   "LDA requires at least 3 groups. Current selection has 2."

## Debugging tip

Capture the response body in the status assertion
(`assert_eq!(status, EXPECTED, "body: {}", ...)`); it immediately exposed that the
"gate" request was actually hitting the merged 3-level file. Read `response.status()`
*before* `into_body()` (body consumes the response).

## Status

- `cargo test --workspace`: all green (API crate 10 passed).
- `cargo fmt` clean; clippy warnings pre-existing in other crates only.