# Phase 2 Group Operations Adapters 2026-09-16

Up: [[../IMPLEMENTATION]]

## Outcome

The transaction-backed group operations (built in
[[Phase_2_Group_Profile_Import_2026-09-14]] as pure storage logic) are now
exposed through both transport adapters over one shared `GroupService`
application core, per Section 10.2's local form:

- **`crates/contracts`** — `GroupSummary` (path, group_id/group_name,
  revision_id, row_count, provenance, role columns; deliberately omits hidden
  `analytical_uuid` values per Section 10.2 display-DTO rule),
  `GroupCandidate` (path + ready flag + optional summary),
  `TransferAction` (`move`/`copy`, snake_case serde),
  `TransferUnitsRequest` (source/destination paths, optional
  `destination_group_name` for new targets, string UUIDs,
  `expected_source_revision` for optimistic concurrency),
  `MergeGroupsRequest` (first source is the target),
  `TransactionResponse` (transaction_id, outputs, deleted_paths).
- **`crates/application/src/groups.rs`** — `GroupService` over
  `FsGroupFileStore`: `new()` runs the Section 6.8 startup recovery before any
  use case; `scan_candidates()` maps `scan_project` candidates to root-relative
  paths with readiness; `validate()` is full validation-on-add;
  `transfer_units()` checks the expected revision (→ `RevisionConflict`),
  parses UUIDs, plans via `plan_move`/`plan_copy` (schema compatibility,
  UUID-multiset, measured-value invariants), bumps revisions `rev-N+1`
  (UUIDv7 fallback for non-numeric revisions), names new destinations with
  `sanitize_group_name`, and executes one journaled `Transaction` whose
  `delete_paths` remove emptied move-out sources; `merge_groups()` merges into
  the first source and deletes the rest.
- **`crates/api`** — `GET /api/v1/groups`, `POST /api/v1/groups/validate`,
  `POST /api/v1/groups/transfer-units`, `POST /api/v1/groups/merge`;
  `store_error_response` maps `RevisionConflict` → 409 `revision_conflict`,
  `SchemaMismatch`/`Invariant`/`Validation` → 422 `validation_error`,
  `Io` → 400 `io_error` through the `ErrorEnvelope`.
- **`crates/desktop` + Tauri shell** — `DesktopGroups` (Mutex-held optional
  `GroupService`; `open_project` runs recovery) with commands
  `scan_group_candidates`, `validate_group_file`, `transfer_units`,
  `merge_groups`, registered alongside the import commands.

## Key discoveries

- `FsGroupFileStore` methods live on the `GroupFileStore` trait — the trait
  must be in scope (`use archaeodash_storage::GroupFileStore`) or every call
  fails with E0599 "method not found in `FsGroupFileStore`".
- Display summaries intentionally exclude hidden identities, so HTTP-level
  transfer tests exercise rejection paths (bad UUID → 422, stale revision →
  409) while UUID-level move/copy invariants are covered at the application
  layer where rows are readable.
- `plan_move` returns `(Option, Option)`: a `None` source successor means
  every row moved out and the executor deletes the source file via
  `delete_paths` — the service must add that path itself.
- `scan_project` returns absolute `PathBuf`s; the service strips the root for
  project-relative DTO paths.

## Verification

`cargo fmt` clean; `cargo clippy --workspace -- -D warnings` clean; full
workspace `cargo test` green: 72 passed, 0 failed (was 61). Tauri shell
(`archaeodash-desktop-app`) compiles.

## Remaining in Phase 2

Delete/unload group route, hosted catalog upload (`POST /projects/{id}/files`
quarantine), `/files/{id}` metadata/download, then Phases 3–6 (goldens #5–#13:
seeded imputation, PCA/LDA via faer with sign alignment, UMAP, clustering,
membership, Euclidean, Explore, multiplot) and Phase 7 (hosted auth).

## Related

- [[Phase_2_Import_HTTP_Tauri_Exposure_2026-09-16]]
- [[Phase_2_Group_Profile_Import_2026-09-14]]
