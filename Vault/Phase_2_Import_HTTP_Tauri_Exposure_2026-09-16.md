# Phase 2 Import HTTP Tauri Exposure 2026-09-16

Up: [[../IMPLEMENTATION]]

## Outcome

Phase 2 remaining item delivered: the import pipeline is now exposed through
both transport adapters, all sharing one application core (Section 1 rule:
desktop never talks to a hidden HTTP server and the HTTP adapter owns no
business logic).

- **`crates/contracts`** — transport-neutral DTOs:
  `ImportPreviewRequest`/`ImportPreviewResponse` (source path, optional group
  column, row count, cleaned columns, `default_id_column` /
  `default_chem_columns` suggestions, partition summary with suggested
  Parquet paths), `ImportRecipeDto` (value-class policies), and
  `ImportCommitRequest`/`ImportCommitResponse`/`CommittedGroup`.
- **`crates/application`** — new `import.rs` with `ImportService`:
  - `resolve()` lexical path containment (Section 6.3 rule: rejects
    absolute paths, `..`, prefix components).
  - `preview()` — `data_loader` semantics, never modifies the source.
  - `commit()` — `partition_by_group` (first-appearance order), one
    `write_group_file` per partition with SHA-256 source provenance and
    `rev-1`, deterministic `_2`/`_3` suffixes for sanitized-name collisions,
    full `validate_group_file` on every published file. Destination
    directories are created only after partition validation, so a rejected
    commit touches no filesystem state.
  - `scan()` — manifest-free candidate discovery via `scan_project`
    (returns absolute paths, sorted, dot-directories skipped).
- **`crates/api`** — `POST /api/v1/imports/preview` and
  `POST /api/v1/imports/commit` with `AppState { import: Arc<ImportService> }`;
  errors map `ImportError::Parse` → 422 `parse_error` and `Io` → 400
  `io_error` through the `ErrorEnvelope` (RFC 9457-style).
- **`crates/desktop` + Tauri shell** — `DesktopImport` (Mutex-held optional
  project root, `open_project`) with `open_import_preview` /
  `commit_group_import` returning `Result<_, String>`; commands registered in
  `apps/desktop/src-tauri`.

## Key discoveries

- `partition_by_group` preserves first-appearance order while the older
  `group_partitions` sorts alphabetically — commit output order follows
  source appearance.
- `scan_project` returns absolute `PathBuf`s, not project-relative paths;
  callers must join against the root for display.
- Private `use` imports in a parent module are not visible through
  `use super::*` in test modules — tests need their own imports.
- Blank cells in *elemental* columns become NA per the recipe; the
  `blank_non_element_label` only applies to non-elemental (descriptive) cells.
- Repo clippy gate is `cargo clippy --workspace -- -D warnings` (no
  `--all-targets`); test modules rely on `unwrap`/`expect` freely.

## Verification

`cargo fmt` clean, `cargo clippy --workspace -- -D warnings` clean, full
workspace `cargo test` green: 61 passed, 0 failed (was 39). New tests cover
preview defaults/partitions, commit per-group publication with provenance and
recipe labels, blank-group rejection without filesystem side effects,
sanitized-name dedupe, path-escape rejection (application + HTTP 422), the
preview→commit HTTP round trip, and the desktop no-project-open guard.

## Remaining in Phase 2

Import/scan routes are local-project form; hosted catalog routes
(`POST /projects/{id}/files` quarantine upload, `/files/{id}` metadata,
`GET /projects/{id}/candidates` pagination) plus the transaction-backed
group operations (`/groups/transfer-units`, `/groups/merge`, etc. over
`FsGroupFileStore`) and the remaining Section 10.2 surface are next, then
Phases 3–6 (goldens #5–#13) and Phase 7 (hosted auth).

## Related

- [[Phase_2_Group_Profile_Import_2026-09-14]]
- [[Implementation_Plan_Revision_And_Split_2026-09-09]]
