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

## Remaining in the hosted data plane

Quota accounting (`storage_quotas`), the async retention sweep for
`.trash`/tombstones, and per-project manifest/transformations/results
sub-namespaces (Sections 6.4/6.9) — each is additive to this catalog.

## Related

- [[Phase_8_Non_Gated_Rehearsals_2026-10-04]]
- [[Phase_7_Hosted_Composition_2026-10-03]]
- [[Phase_2_Source_File_Catalog_2026-09-17]]
