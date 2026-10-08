# Hosted Transformation Definitions (2026-10-08)

Implements the first of the two "remaining in the hosted data plane" items
from [[Phase_8_Hosted_File_Catalog_2026-10-04]]: named transformation
definitions for hosted projects (Sections 6.4/6.5/10.2). The quota policy
revision mechanism remains open.

## What was built

- **Migration `0007_transformations.sql`** — the Section 6.5
  `transformations` catalog table: `transformation_id` (UUIDv7), project FK,
  project-unique active `name`, integer `revision`, `object_key`/`sha256`/
  `bytes` for the definition JSON in the file store, summary columns
  (`transform_method`, `imputation_method`, `ratio_count`) so listing never
  reads objects, and `input_checksums JSONB` reserved for the future hosted
  apply route. No analytical values — the Section 14.3.2 schema audit test
  now expects nine tables.
- **`ControlStore`** — `upsert_transformation` (transactional: ownership
  re-checked inside, name conflict serializes with `FOR UPDATE`, revision
  bumps monotonically, returns the replaced object key for trash routing),
  `list_transformations`, `get_transformation`,
  `get_transformation_by_name`, `soft_delete_transformation`. Foreign,
  deleted, and unknown IDs are uniformly `None` (no existence oracle).
- **`HostedFileStore`** — `write_definition_object` (staged write + fsync +
  atomic rename into
  `users/<uid>/projects/<pid>/transformations/<tid>/<revision-uuid>.json`
  with SHA-256), `read_object`, and a generalized `trash_object` (the file
  delete path now uses it too).
- **Hosted routes** — `POST/GET /api/v1/projects/{id}/transformations` and
  `GET/DELETE /api/v1/projects/{id}/transformations/{transformation_id}`.
  Session + CSRF on writes; 256 KiB body cap (413); definition validation
  reuses the desktop `TransformService` rules (now exposed as
  `validate_definition`, with `StoredTransformation` shared so desktop and
  hosted envelopes are identical); replace preserves the original
  `created_at_unix_secs` (desktop upsert parity); GET checksum-verifies the
  object against the catalog row before parsing. Definition bytes are not
  quota-counted yet — deliberately deferred to the quota-policy-revision
  slice, noted in code.
- **Retention fix (same day):** the first cut trashed replaced-revision
  objects, but no catalog row references a replaced revision — the Section
  6.9 sweep (which deletes *tombstoned* rows) could never find them, so
  every same-name save leaked an unsweepable object. Replaced revisions are
  now removed outright (desktop upsert parity: one name-keyed JSON, no
  revision history; a crash between commit and removal orphans an
  undiscoverable object, the same guarantee class as the file sweep
  window). Deleted definitions keep tombstone → trash, and the hourly sweep
  gained `sweep_expired_transformations` so their trash objects are purged
  under `AUTH_RETENTION_DAYS` like file tombstones. Verified by a store
  cutoff-scoping test and an HTTP object-lifecycle test (replace removes
  the previous bytes; delete moves them to `.trash/`).
- **Client transport** — `HostedTransformationsService`
  (`save`/`list`/`get`/`delete`) on the HTTP transport; Tauri transport
  unaffected (desktop keeps its local-directory store).

## Verification

- 272 workspace lib tests green with live Postgres (was 271): store-level
  scoping/revision/tombstone test, file-store object layout test, and a
  full HTTP round trip (401 without session, 403 without CSRF, 201 create →
  200 replace with revision 2 and stable ID, list, checksum-verified get,
  stranger 404s, blank-name 422, oversize 413, delete 204 then 404).
- 45 client tests green, tsc clean, vite build green.
- `scripts/e2e/auth-security.mjs` gained section 6c: an in-page fetch
  vertical over the real API (create → list → get → replace → invalid 422 →
  foreign-project 404 → delete 204/404) after the CSV upload; two
  consecutive clean runs against the live stack.
- `deploy/restore.sh` schema check updated 8 → 9 tables and re-verified
  live (backup → restore into a fresh DB → watermark + manifest + readiness
  checks pass, with transformation objects in the file-store manifest).
- `cargo fmt` clean; clippy introduces no new warnings (remaining
  test-only `expect`/`unwrap` lints are pre-existing).
- Dev-environment fix: `apps/web/vite.config.ts` now reads the API proxy
  target from `AUTH_API_PORT` (default 8787 unchanged) so the e2e can run
  when 8787 is occupied.

## Deliberately out of scope here

- Applying definitions against hosted group files (the hosted analysis
  surface) — `input_checksums` is reserved for it.
- Counting definition bytes in `storage_quotas` (quota policy revision).
- A hosted `/projects` UI panel for definitions (the transport service and
  routes are ready for it).

## Related

- [[Phase_8_Hosted_File_Catalog_2026-10-04]]
- [[Phase_8_Non_Gated_Rehearsals_2026-10-04]]
- [[Phase_7_Hosted_Composition_2026-10-03]]
