# Phase 2/4 Descriptive Edit And Duplicate Group — 2026-09-19

Related: [[Phase_4_Explore_Views_Golden_12_2026-09-19]], [[Phase_2_Group_Delete_Route_2026-09-17]]

## Summary

Landed the deferred group-operation surface (IMPLEMENTATION.md Phase 2 line
233-234 and Phase 4 "descriptive-edit table with elemental columns locked",
per docs/implementation/10-hosted-http-api.md section 10.2 and
13-source-disposition.md `R/updateCurrent.R`): batch hidden-UUID-addressed
descriptive edits and whole-group duplication, end to end through storage,
application, HTTP, and Tauri adapters.

## What was built

- `crates/storage`: `TransactionAction::PatchDescriptive` and
  `TransactionAction::DuplicateGroup` join the journal enum; recovery handles
  them generically (single-output publish, no delete paths).
- `crates/application` `GroupService::patch_descriptive_values`: expected-
  revision guard, descriptive-column lock (elemental/identity columns rejected
  with "not a descriptive column"), unknown-UUID abort before any write, one
  journaled transaction per batch, revision bumps to `rev-(N+1)`, measured-
  elemental checksum precondition inherited from the executor.
- `GroupService::duplicate_group`: new destination path (default
  `groups/<sanitized-name>.parquet`), refuses an existing destination and a
  same-path duplicate, preserves analytical UUIDs and source lineage by
  default (`preserve_uuids: false` mints fresh UUIDv7s), `rev-1` revision.
- Contracts: `PatchDescriptiveValuesRequest`/`DescriptiveEdit`,
  `DuplicateGroupRequest` (serde-default `preserve_uuids = true`); both
  return the existing `TransactionResponse`.
- HTTP: `PATCH /api/v1/groups/descriptive-values`,
  `POST /api/v1/groups/duplicate` + round-trip/conflict/lock tests.
- Desktop: `DesktopGroups::patch_descriptive_values`/`duplicate_group` and the
  `patch_descriptive_values`/`duplicate_group` Tauri commands registered in
  `generate_handler!` (Section 10.4 names).

## Decisions

- **Unload is still deferred**: `DELETE /groups/{id}` covers the local
  Phase-2 form; true unload/soft-delete needs the workspace/manifest catalog
  (contracts doc already says "'unload' arrives with the workspace/manifest
  layer"). Phase 7 concern.
- **Patch response carries the updated `GroupSummary`** (with the new
  revision) so clients re-read nothing to continue editing.
- UMAP (golden 7) remains the next Phase 4 slice: class-D parity per
  [[Implementation_Plan_Open_Question_Recommendations_2026-09-09]] §17.1
  item 3 - seeded determinism plus k-NN overlap/trustworthiness metrics
  against the three seeded `umap::umap`-naive oracle replays, never
  coordinate equality. The backend spike decision (umap_rs crate vs
  reimplementation vs experimental flag) is still an open Phase 0 exit
  criterion and should be recorded before that slice starts.

## Verification

Workspace tests 42 suites green (including the new application tests
`patch_descriptive_values_edits_by_uuid_and_bumps_revision`,
`duplicate_group_preserves_uuids_and_lineage`, and the API round-trip
`descriptive_patch_and_duplicate_round_trip_over_http`); `tools/cargo-lint.sh`
green; Tauri shell compiles.

## Related

- [[Phase_2_Group_Delete_Route_2026-09-17]]
- [[Phase_4_Explore_Views_Golden_12_2026-09-19]]
