Up: [[Index]]

# Hosted multi-user hardening (2026-10-28)

Step 4 of [[Hosted_Login_Integration_Plan_2026-10-28]], on top of [[Hosted_Project_Workspaces_2026-10-28]] and [[Hosted_Single_Image_2026-10-28]]. Not committed. P4–P6 fixed on 2026-10-29 ([[Interaction_Log_2026-10-29]]).

## Route audit (read-only subagent, then fixes)
Every `root_router` route was traced from its path-bearing fields into the services.
- **Confined already:**
  - jobs and clustering (`ClusterService::group_path`: lexical check, then canonicalize plus `starts_with`);
  - imports (`ImportService::resolve`, `normalize_folder`);
  - file upload (`resolve_contained`); files by ID (UUID only);
  - group rows, validate, transfer, batch transfer, merge, duplicate, copy, apply-clusters, rename, columns, folders and delete (`FsGroupFileStore::resolve`, `validate_folder_name`, symlink checks where noted);
  - transformations by name (`sanitize_group_name`);
  - preferences (a fixed file); caches (SHA-256 names).
- **Exports** write nothing on the server; they return bytes. No route reveals folders or logs, or takes host paths.

### Fixed
- **P1, critical: reads outside the project, including other users' files.**
  - Problem: `CombinedInput::read_local` (`crates/application/src/combined.rs`) did `read_group_file(root.join(rel))` with no check. An absolute path or `../../<user>/projects/<project>/workspace/...` could read any group Parquet file on the host. That reached ordination (PCA, LDA, UMAP), all four explore routes, the measured, transformed and PCA-score exports, and transformations/apply.
  - Fix: a new `contained_path(root, rel)`. It accepts only plain relative components (`is_plain_relative`), then canonicalizes and requires the result to stay inside the canonical root, so symlinks out are refused too. The refusal is `Validation path_outside_project` (HTTP 422).
  - Also: `PoolScope::new` (`prepared.rs`) skips non-plain paths, so the pooled-imputation folder listing can't scan outside; LDA builds its pool and cache key before reading. The ordination `cache_file` metadata lookup uses `contained_path` too.
  - Missing files keep their previous internal-error behaviour (`lda_missing_group_file_is_internal_error`).
- **P2, high: absolute server paths in group listings.** `GroupService::to_candidates` strips the root as passed, while the store scans the canonical root. A relative or symlinked `AUTH_FILE_STORE_DIR` would therefore return absolute host paths and break group operations. Fix: `WorkspaceRegistry::router` opens every workspace through its canonical path.
- **P3, medium: server paths in error messages.** `data-io` `loader.rs` said `cannot read <full path>`; it now names only the file.

### Fixed on 2026-10-29 (were open, low)
- **P4: links out of the project.**
  - One resolver, `resolve_in_project` (`crates/file-store-fs`), now serves the group store (`FsGroupFileStore::resolve`), imports (`ImportService::resolve`) and uploads (`resolve_contained`).
  - After the lexical check it inspects each existing component: a symbolic link or Windows junction is allowed only if it resolves inside the project. Dangling links are refused too, since writing through one would create its target wherever it points.
  - Links inside the project keep working. On the desktop app, a link inside a project that points elsewhere on the machine is now refused; analyses already refused those since P1.
  - Scans (`scan_dirs` in `data-io`) skip links that lead outside the folder scanned, so a listing never shows files from elsewhere. (Clustering, batch transfer and apply-clusters still refuse every link, as before.)
- **P5: writes into hidden folders.** `execute` refuses any output or delete path with a hidden component (such as `.archaeodash/`) or not ending in `.parquet` (`Validation`, HTTP 422). Uploads refuse hidden components.
- **P6: upload race.** Upload now publishes with `hard_link` (fails if a file appeared at the target) instead of `rename`, then drops the quarantine copy. On failure the record is removed as well. The early "already exists" check stays for a clear message.
- Tests: links out, in and dangling (Unix); hidden and non-Parquet transaction outputs; hidden upload paths; no-clobber promote and quarantine cleanup; upload through a link out.

### Still noted
- Preferences live in the project folder, so they are per project, not per user. Account-level preferences use the root `/api/v1/preferences`.

## Limits (`crates/api/src/workspaces.rs`, `WorkspaceLimits`; env in `bin/hosted.rs`, documented in `hosted-stack.yml`)
- **Body limit:**
  - `DefaultBodyLimit` on every workspace router, `AUTH_MAX_REQUEST_BYTES` (default 64 MiB). Axum's 2 MB default would otherwise reject larger spreadsheets.
  - Apache `LimitRequestBody` must allow at least this much.
- **Analysis slots:**
  - CPU-heavy inner routes (ordination, cluster, euclidean, explore, exports, imports, transformations apply and ratios) need one per-user slot (`AUTH_ANALYSIS_SLOTS_PER_USER`, default 2) and one shared slot (`AUTH_ANALYSIS_SLOTS`, default half the CPUs). Some of these handlers run on the async runtime's threads, which is why the shared default leaves half free.
  - Requests wait up to `AUTH_ANALYSIS_WAIT_SECS` (60), then get 503 `server_busy`.
  - Cheap reads (scan, rows, validate) are not limited.
- **Background jobs:** each project gets `AnalysisJobs::with_limits` with 1 worker and 4 queued jobs (`AUTH_JOB_WORKERS`). The desktop default is 2 and 16. New `AppState::open_with_jobs`.
- **Storage quota:**
  - Before the inner `POST` routes that add data (files, imports/commit, groups duplicate, copy, apply-clusters, columns, transformations save), the user's total is compared with `AUTH_QUOTA_BYTES`. The total is all their workspace folders (caches included) plus catalog bytes. At or over the limit the request gets 413 `quota_exceeded`.
  - Reads, analyses, moves and deletes keep working, so a user can always clean up.
  - It is a soft limit: one write can overshoot by at most its own size, which the body limit bounds.
- **Usage:** `GET /api/v1/quota` adds `workspace_bytes`. The client's `QuotaUsage` has an optional `workspace_bytes`, and the Projects page shows "X of Y used" again (`usageText`). A `quota_exceeded` error reads "Your storage is full…".
- **Project delete:** middleware `remove_deleted_workspace` closes the project's router and removes its workspace folder after the catalog `DELETE /api/v1/projects/{id}` returns 204. Removal is immediate (no trash). There is no project-delete button in the UI yet.

## Verification
- `check.sh rust` on the full workspace (fmt, all tests, clippy) and `check.sh parity` pass. `archaeodash-api` has 69 tests and `archaeodash-application` 126.
- The DB-backed API tests pass against a throwaway Postgres on home2 (removed afterwards).
- `check.sh web`: 312 tests pass.
- New tests:
  - route classification;
  - analysis slots (per-user cap, shared cap, release);
  - usage counts only that user's workspaces, and remove;
  - the body limit (413 and 200);
  - DB-backed: quota refuses the second upload while reads work, the quota route reports workspace bytes, and project delete removes the workspace (then 404);
  - containment unit tests (plain paths, `..`, absolute, symlink out, `read_local` refusal);
  - API: PCA and export with `..` and an absolute path into another user's workspace get 422 `path_outside_project`, with no server path added to the message;
  - web `usageText`.
