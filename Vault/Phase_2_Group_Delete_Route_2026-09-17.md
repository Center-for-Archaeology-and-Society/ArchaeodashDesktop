# Phase 2 Group Delete Route 2026-09-17

Up: [[../IMPLEMENTATION]]

## Outcome

The Section 10.2 `DELETE /groups/{id}` route (local Phase-2 form) now exists
through both transport adapters over the shared `GroupService`:

- **`crates/contracts`** — `DeleteGroupRequest` (`path`, `expected_revision`,
  `confirm_path`). Destructive-command rule per Section 10.4: the client must
  confirm the exact path; unload-availability/manifest semantics arrive with
  the workspace layer, so delete is currently the whole operation.
- **`crates/storage`** — new `TransactionAction::DeleteGroup` variant. The
  existing Section 6.8 executor already handles empty `outputs` +
  non-empty `delete_paths`: preconditions, journal, backup of the original
  into the bounded `.archaeodash/history` archive, delete, commit, cleanup.
- **`crates/application/src/groups.rs`** — `GroupService::delete_group()`:
  rejects mismatched `confirm_path` (Invariant → 422), checks
  `expected_revision` (→ 409 `RevisionConflict`), then runs one journaled
  transaction returning `action: "delete_group"` and the deleted path. The
  exhaustive `TransactionAction` match in `transfer_units` gained the
  `DeleteGroup` arm (compile error caught by clippy).
- **`crates/api`** — `DELETE /api/v1/groups/{*path}` (axum 0.8 wildcard syntax)
  with `expected_revision`/`confirm_path` query parameters, reusing
  `store_error_response` (409 conflict / 422 validation_error / 400 io_error).
- **`crates/desktop` + Tauri shell** — `DesktopGroups::delete_group` command
  body plus the registered `delete_group` Tauri command.

## Key discoveries

- After a committed transaction the `.archaeodash/transactions` *root* remains
  (only the per-tx directory is removed); assertions must check it is empty,
  not absent — same pattern the file-store tests use.
- A rejected transaction (e.g. copy with empty UUID selection) must not be
  assumed to have created its destination in later assertions.
- `serde_json::Error` lacks `PartialEq`, so DTO round-trip assertions must
  `.unwrap()` the parse instead of comparing against `Ok(..)`.

## Verification

`cargo fmt` clean; `cargo clippy --workspace -- -D warnings` clean; full
workspace `cargo test` green: 78 passed, 0 failed (was 72). Tauri shell
(`archaeodash-desktop-app`) compiles. New tests cover: application delete
success/confirmation-mismatch/stale-revision, HTTP 422/409/200/400 sequence,
and desktop delete including the rejected-copy path assertion.

## Remaining in Phase 2

Hosted catalog upload (`POST /projects/{id}/files` quarantine),
`/files/{id}` metadata/download, then Phases 3–6 (goldens #5–#13: seeded
imputation, PCA/LDA via faer with sign alignment, UMAP, clustering,
membership, Euclidean, Explore, multiplot) and Phase 7 (hosted auth).

## Related

- [[Phase_2_Group_Operations_Adapters_2026-09-16]]
- [[Phase_2_Import_HTTP_Tauri_Exposure_2026-09-16]]
