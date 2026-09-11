# IMPLEMENTATION Section 10 - Hosted HTTP API

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 10. Hosted HTTP API

Use `/api/v1`; JSON for commands/metadata, Arrow IPC stream for full/large tables, and RFC 9457-style problem details for errors. Every response includes a request/correlation ID. Generated OpenAPI is the source for TypeScript transport types.

### 10.1 Auth and preference routes

| Method/path | Behavior |
|---|---|
| `POST /auth/register` | validate normalized username/email/password and consent version; create unverified account; rate limit; send verification |
| `POST /auth/verify` | consume one-time 24-hour token; generic safe errors |
| `POST /auth/login` | verify password/email status; rotate session; set HttpOnly cookie; optional remembered expiry |
| `POST /auth/logout` | revoke current session and expire cookie |
| `POST /auth/logout-all` | revoke all user sessions |
| `GET /auth/session` | return minimal principal and CSRF bootstrap state |
| `POST /auth/password-reset/request` | enumeration-resistant generic response; persistent throttling |
| `POST /auth/password-reset/confirm` | consume one-time token, update hash, revoke sessions/tokens |
| `GET/PUT /preferences` | typed allowlisted preference keys only |

### 10.2 Project/file/group/workspace routes

| Method/path | Behavior |
|---|---|
| `POST /projects` | create an authorized hosted project and initial manifest |
| `GET /projects` | authorized project/file catalog with pagination, quota, and active group revision summaries |
| `GET /projects/{id}` | authorized project summary, settings, and current revision |
| `GET /projects/{id}/candidates` | recursively inspect logical `.parquet` paths and return metadata-readiness states; never treat catalog membership as eligibility |
| `POST /projects/{id}/candidates/refresh` | rescan footer metadata and invalidate validation records for changed files |
| `GET /projects/{id}/export` | stream a portable bundle containing selected group files, metadata, and optional selected sources; caches omitted |
| `POST /projects/{id}/files` | upload a file to a user-selected in-project logical path via bounded quarantine; return staged file ID |
| `GET /files/{id}` | authorized metadata, checksum, format capabilities, storage version, and parse state |
| `GET /files/{id}/download` | stream file bytes with a safe display filename after authorization |
| `POST /files/{id}/imports/preview` | validate/parse a source spreadsheet without modifying it; return expiring role/group preview |
| `POST /imports/{preview_id}/commit` | atomically publish one validated group Parquet file per group or a single named group when no group column exists |
| `DELETE /files/{id}` | soft delete after reference and expected-revision checks; retention cleanup is asynchronous |
| `POST /projects/{id}/groups/validate` | full validation of an in-project metadata-ready Parquet path; return structured report without adding on failure |
| `POST /projects/{id}/groups` | add/load a fully validated group candidate; manifest/catalog indexing is an effect, not a prerequisite |
| `GET /groups/{id}` | path, metadata/schema/provenance, editability, revision, and validation report |
| `GET /groups/{id}/rows` | authorized paged/sorted/filtered projection; Arrow option; omit `analytical_uuid` from ordinary display DTOs |
| `PATCH /groups/{id}/descriptive-values` | batch hidden-UUID-addressed descriptive edits with expected revision; elemental roles rejected |
| `PATCH /groups/{id}/metadata` | validated group name/provenance/editability update with expected revision |
| `POST /groups/{id}/editable-clone` | clone a read-only reference with new `GroupId`, retaining full lineage |
| `POST /groups/{id}/enable-editing` | explicitly enable in-place editing when permitted, recording divergence warning/actor |
| `POST /groups` | create an empty compatible group at a selected project-relative path |
| `POST /groups/{id}/duplicate` | duplicate group; preserve UUIDs and lineage by default |
| `POST /groups/transfer-units` | atomically move/copy analytical units among groups using hidden UUIDs and measured-value invariants |
| `POST /groups/split` | create groups from selected analytical units in one journaled transaction |
| `POST /groups/merge` | merge compatible groups after schema/unit and duplicate-UUID checks |
| `DELETE /groups/{id}` | confirmation token + expected revision; unload/soft-delete/archive according to explicit choice |
| `POST /workspaces` | create workspace from one/many fully validated current group revisions |
| `GET /workspaces/{id}` | schema/source map/status |
| `POST /workspaces/{id}/descriptive-columns` | add/overwrite a descriptive column; reject elemental or identity roles |
| `PATCH /workspaces/{id}/assignments` | translate assignment into atomic analytical-unit transfers among group files |
| `DELETE /workspaces/{id}` | clear ephemeral workspace |

### 10.3 Transformation/analysis/job/export routes

| Method/path | Behavior |
|---|---|
| `POST /workspaces/{id}/transformations/validate` | structured prerequisites/warnings/cost estimate |
| `POST /workspaces/{id}/transformations` | save definition and enqueue run; explicit overwrite revision |
| `GET /workspaces/{id}/transformations` | lightweight definition list; no transformed matrix hydration |
| `GET /transformations/{id}` | definition, seed, versions, and ordered input group checksums |
| `POST /transformations/{id}/run` | revalidate groups and recompute transforms/permutations/results on demand |
| `DELETE /transformations/{id}` | soft delete definition and unreferenced explicit result refs |
| `POST /analyses/cluster` | validated cluster job |
| `POST /analyses/membership` | membership job |
| `POST /analyses/euclidean` | nearest-match job |
| `GET /results/{id}` | metadata and small ephemeral or retained result |
| `GET /results/{id}/data` | authorized Arrow stream/page |
| `POST /exports` | create measured-data/result/plot export job; calculated data may not receive group-file metadata |
| `GET /exports/{job_id}/download` | short-lived authenticated stream; no public permanent URL |
| `GET /jobs/{id}` | status/progress/result/error |
| `POST /jobs/{id}/cancel` | cooperative cancellation |
| `GET /jobs/{id}/events` | SSE progress/stage stream |
| `GET /health/live` | process liveness only |
| `GET /health/ready` | control DB/user-file-store/schema/job readiness without sensitive details |

### 10.4 Desktop command surface

Expose equivalent use cases with typed Tauri commands such as `create_project`, `open_project`, `scan_group_candidates`, `validate_group_file`, `add_group_file`, `unload_group`, `reveal_project_file`, `open_import_preview`, `commit_group_import`, `create_group`, `duplicate_group`, `move_analytical_units`, `copy_analytical_units`, `split_group`, `merge_groups`, `enable_group_editing`, `clone_editable_reference`, `patch_descriptive_values`, `load_workspace`, `save_project_settings`, `export_group`, `save_transformation`, `run_transformation`, `run_cluster`, `run_membership`, `run_euclidean`, `cancel_job`, `choose_export_path`, and `export_result`. Commands accept IDs and DTOs, not arbitrary SQL. Rust path-taking commands accept explicit native-dialog selections or canonicalized paths below the opened project root; webview code does not receive a generic filesystem traversal interface. Destructive file commands require exact-path confirmation and are never implied by unloading a group.
