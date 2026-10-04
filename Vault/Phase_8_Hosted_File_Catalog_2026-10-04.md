# Phase 8 — Hosted File Catalog and Per-User Object Store (2026-10-04)

## Context

The hosted project catalog (Section 10.2) landed in commit `781d7cc`, but hosted
users still had no file storage: `crates/application/src/files.rs` carried the
stale note that "the hosted `UserFileStore`/catalog replaces this layout in
Phase 7", and Section 6.4's per-user namespace existed only as a compose volume
mount. This slice implements the hosted file data plane end to end.

## What was built

- **Migration `0005_file_catalog.sql`** — `files` table per the Section 6.5
  catalog-row list: identity, ownership (FK to `projects`), logical path
  (unique among live rows per project), kind, display filename, opaque object
  key (unique), SHA-256, media type, extension, bytes, lifecycle state
  (`staged`/`published`/`deleted`), parse error, tombstone. No analytical data.
- **`ControlStore` file methods** — `insert_file` performs the ownership check
  inside the INSERT (`INSERT … SELECT … FROM projects WHERE user_id = $13`),
  so a foreign or deleted project rejects atomically. `get_file`/`list_files`
  join through `projects` and filter tombstones: foreign, unknown, and deleted
  IDs are uniformly `None` (no existence oracle). `soft_delete_file` returns
  the object key so the caller can sweep the bytes.
- **`ControlError::NotFound`** variant mapped to 404 in the API error mapper.
- **`HostedFileStore`** (`crates/api/src/hosted_files.rs`) — local-FS backend:
  objects at `users/<user-id>/projects/<project-id>/files/<file-id>/1`
  (UUIDv7 keys, never client strings), staging write + `sync_dir` + atomic
  rename promote, SHA-256, CSV parse-check via `data_loader` (tsv/xlsx
  deferred as on desktop), `discard_staged` for catalog rejections, trash
  routing on delete. Logical-path validation: relative, no `..`/empty/`.`/
  absolute components, csv/tsv/xlsx allowlist, display name = last segment.
- **HTTP routes on the hosted router** — `POST /api/v1/files` (upload,
  session + CSRF), `GET /api/v1/files?project_id=…` (list), `GET/DELETE
  /api/v1/files/{id}`, `GET /api/v1/files/{id}/download` (Content-Type from
  allowlist, display filename only in quoted `Content-Disposition`, never in
  a URL or key). Errors: 401 unauthenticated, 403 CSRF, 404 uniform
  not-found, 413 size, 422 invalid path/format.
- **Binary/compose/runbook** — `AUTH_FILE_STORE_DIR` env (default
  `./data/user-files`; compose sets `/data/user-files` on the `file-store`
  volume), documented in `docs/operations/hosted-deployment.md`.
- Stale Phase-7 comment in `files.rs` updated to point at the hosted form.

## Key discoveries

- The Section 6.5 `files.state` CHECK constraint (`staged/published/deleted`)
  is a lifecycle state, not the parse outcome — parse success/failure surfaces
  through `parse_error` and the API response's `parse_state`, keeping the
  catalog row schema-clean. Found via a live CHECK violation during e2e.
- `FileRow` carries `user_id` from the JOIN (`p.user_id`), not a `files`
  column — ownership is expressed by foreign key only, per Section 6.5.
- Object keys are validated by construction (server-generated UUIDs), but the
  object path join stays defensive against catalog corruption.

## Verification

- 267 workspace lib tests green with live Postgres (was 261): store-level
  catalog scoping test (8 scenarios: ownership-rejected insert, unique active
  path, stranger sees nothing, tombstone frees the path, object key returned
  on delete), object-store staging tests (opaque key layout, no orphan on
  oversize, parse-failure-not-fatal, path validation), HTTP 401 negative.
- Live stack e2e (hosted binary + Postgres 16 container): register → verify →
  login → create project → upload CSV (201, correct metadata/checksum) →
  list → download (bytes match) → stranger get/download/delete all 404 →
  owner delete 204 → repeat delete 404 → download after delete 404 → object
  in `.trash/`; no-CSRF 403, `../escape.csv` 422, `.exe` 422.
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --lib` zero
  warnings (new code; pre-existing test-only `expect/unwrap` lints in
  `--all-targets` were already at ~298 before this slice).

**Follow-up (same day, commit `ce3407f`):** per-user storage quota
(Section 6.9) is live — migration `0006_storage_quotas.sql`
(logical/reserved bytes, file count, policy revision per owner),
`ControlStore::reserve_quota` with an atomic limit guard, `reconcile_quota`
(Commit/Release/Remove), upload route reserving before staging and
reconciling on every outcome, delete reconciling tombstoned bytes, and
`AUTH_QUOTA_BYTES` (default 1 GiB) in the binary, compose stack, and
runbook. Live e2e: fit → 201, delete → 204 frees quota, over-quota → 413
`quota_exceeded` with no reserved-byte leakage. 269 workspace tests green.

**Retention sweep (Section 6.9, commit `82ce641`):** the remaining
tombstone cleanup is implemented — `ControlStore::sweep_expired_tombstones`
atomically deletes rows tombstoned past the cutoff and returns ownership +
object key; `HostedFileStore::purge_trash_object` removes the trash bytes;
the hosted binary runs the sweep hourly under `AUTH_RETENTION_DAYS`
(default 30). Crash between DB delete and FS purge can only orphan an
inaccessible trash object, never a discoverable one.

**Web client slice (commit `f04bb64`):** `HostedProjectsPage` at
`/projects` — list/create projects, per-project file table with upload
(logical path + picker), parse state, and delete; stable error-envelope
messages; nav entry following the Account pattern. Client transport
exports `HostedProjectsService`/`HostedFilesService`. Web tests 101
green, tsc clean, vite build green; live smoke of the full vertical
(register → login → project → upload → list) passed.

**Quota route (Section 10.2, commit `b6bd95b`):** `GET /api/v1/quota`
exposes the signed-in user's storage accounting (logical/reserved bytes,
file count, effective limit; zeros before first upload) and the web
Projects page shows live usage, refreshed after create/upload/delete.

**Restore drill updated for the new tables (commit `0b433ad`):** the
drill's schema check still counted the original 5 control-plane tables,
so a complete restore of a current database now fails as "incomplete
(5/8)". Fixed to 8 (adds `projects`, `files`, `storage_quotas`), aligned
the backup/restore scripts on `AUTH_FILE_STORE_DIR` (alias
`USER_FILE_STORE_DIR`), and fixed the manifest path parse (sha256sum
text mode is `<hash>␠␠<path>`; the old cut left a leading space so every
drift check mis-opened the file). Verified live: backup of a store with
a real uploaded object, restore into a fresh DB passes the 8-table +
manifest checks, a corrupted object fails with `file-store drift`, and
the restored DB passes the API readiness gate with
`AUTH_APPLY_MIGRATIONS` unset while serving the restored project
(login 200, project listing intact).

## Remaining in the hosted data plane

Per-project manifest/transformations/results sub-namespaces (Section 6.4)
and quota policy revisions are additive to this catalog.

## Related

- [[Phase_8_Non_Gated_Rehearsals_2026-10-04]]
- [[Phase_7_Hosted_Composition_2026-10-03]]
- [[Phase_2_Source_File_Catalog_2026-09-17]]
