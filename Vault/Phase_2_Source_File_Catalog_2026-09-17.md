# Phase 2 Source File Catalog 2026-09-17

Up: [[../IMPLEMENTATION]]

## Outcome

The remaining Phase 2 file-catalog surface (Section 10.2 `POST
/projects/{id}/files` bounded-quarantine upload, `GET /files/{id}`,
`GET /files/{id}/download`, `DELETE /files/{id}`) now exists in its local
Phase-2 form through both transport adapters over one shared
`SourceFileService` application core. The quarantine record directory
`.archaeodash/quarantine/` under the opened project root is the file catalog
until the hosted control plane replaces it in Phase 7.

- **`crates/data-io`** — new `ImportError::NotFound` (HTTP 404) and
  `ImportError::Limit` (HTTP 413; Section 17.1.5 explicit-limits rule)
  variants; `StoreError` `From` impl maps both while preserving messages.
- **`crates/contracts`** — `StagedFile` (file_id, path, size_bytes, sha256,
  format, parse_state `parsed`/`parse_failed`/`deferred`, parse_error,
  deleted tombstone), `FileUploadRequest` (path + raw bytes, desktop IPC
  form), `FileDownload` (metadata + bytes).
- **`crates/application/src/files.rs`** — `SourceFileService`: upload stages
  bytes into quarantine under a 256 MiB bound (test seam shrinks it), checks
  the extension allowlist (csv/tsv/xlsx, Section 7.1) and lexical path
  containment, rejects existing targets, computes SHA-256, parse-checks CSV
  (TSV/XLSX deferred until their Section 7.1 adapters land), writes the
  record, then promotes atomically by rename; `metadata`, `download` (refuses
  deleted), and `delete` (soft: bytes move to quarantine trash, tombstone
  kept). File IDs are UUID-validated so no client string reaches the
  filesystem.
- **`crates/api`** — `POST /api/v1/files?path=…` (raw body), `GET`/`DELETE
  /api/v1/files/{id}`, `GET /api/v1/files/{id}/download` with a sanitized
  Content-Disposition filename; error envelope mapping 404 `not_found` and
  413 `limit_exceeded`.
- **`crates/desktop` + Tauri shell** — `DesktopFiles` and commands
  `upload_source_file`, `source_file_metadata`, `download_source_file`,
  `delete_source_file`.

## Key discoveries

- The csv reader with `flexible(true)` tolerates ragged rows and even
  unterminated quotes in tested cases; invalid UTF-8 reliably fails the
  string-record parse, which the `parse_failed` state needs for tests.
- A committed group transaction leaves the `.archaeodash/transactions` root
  present (only per-tx directories are removed); assert emptiness, not absence.
- Rust `SourceFileService::new(...).with_max_upload_bytes(8)` chained into a
  local binding named `service` shadowed the `service()` test helper — E0618.

## Verification

`cargo fmt` clean; `cargo clippy --workspace -- -D warnings` clean; full
workspace `cargo test` green: 91 passed, 0 failed (was 78). Tauri shell
(`archaeodash-desktop-app`) compiles. New tests: 5 application (round trip,
rejections, size bound, parse states, soft delete, malformed IDs), 2 HTTP
(full upload→metadata→download→delete→404 cycle and escape/format/unknown-ID
status codes), 1 desktop round trip with no-project error.

## Remaining in Phase 2

Nothing code-blocking in the local adapter surface; hosted catalog concerns
(per-user namespaces, reference/expected-revision checks on delete,
asynchronous retention cleanup) move with the Phase 7 control plane. Next:
Phases 3–6 (goldens #5–#13: seeded imputation, PCA/LDA via faer with sign
alignment, UMAP, clustering, membership, Euclidean, Explore, multiplot) and
Phase 7 (hosted auth).

## Related

- [[Phase_2_Group_Delete_Route_2026-09-17]]
- [[Phase_2_Group_Operations_Adapters_2026-09-16]]
