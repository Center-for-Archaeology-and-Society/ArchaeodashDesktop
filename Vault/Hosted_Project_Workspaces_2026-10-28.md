Up: [[Index]]

# Hosted project workspaces behind sign-in (2026-10-28)

Step 1 of [[Hosted_Login_Integration_Plan_2026-10-28]], design A: the analysis API now runs per hosted project, behind the hosted sign-in. This is not committed and not deployed.

## What changed
- **`crates/api/src/workspaces.rs` (new):**
  - `WorkspaceRegistry` opens one analysis router per `(user, project)`. Its project folder is `AUTH_FILE_STORE_DIR/users/<user-uuid>/projects/<project-uuid>/workspace/`, created on first use and kept in a least-recently-used cache (`DEFAULT_OPEN_WORKSPACES` = 64).
  - Opening happens under the lock, so a project never gets two job lists.
  - Closing a router drops that project's in-memory job list.
- **Route:** `/w/{project_id}/api/v1/...` (`WORKSPACE_ROUTE`), handled by `workspace_dispatch`. A request must pass these checks in order:
  1. a live session, otherwise 401;
  2. on POST, PUT, PATCH and DELETE, the CSRF token, otherwise 403 (via `require_session`);
  3. ownership through `get_project`, otherwise **404**. Other users' projects, deleted projects and unknown IDs look the same.
- **Forwarding:** the inner request gets the raw `/api/v1/...?query` URI, so percent-encoded group paths survive. Paths outside `/api/v1/` are 404.
  - **Pitfall:** the request extensions must be cleared before forwarding. Otherwise axum passes the outer route's path parameters (`project_id`, `rest`) to inner `Path` extractors, which then fail with 500 ("Expected 1 but got 3"). The isolation test caught this.
- **`crates/api/src/lib.rs`:** new `AppState::open(root)` builds all ten services for a folder. `examples/local.rs` now uses it.
- **`crates/api/src/hosted.rs`:**
  - `HostedState.workspaces` (new field);
  - the route is merged into `hosted_router`, so it has request IDs and security headers;
  - `internal_error`, `unauthorized` and `json_error_envelope` are now `pub(crate)`;
  - the test helpers build a registry.
- **`crates/api/src/bin/hosted.rs`:** the registry uses the same `AUTH_FILE_STORE_DIR`. New optional env `AUTH_OPEN_WORKSPACES` sets the cap.

## Tests
New tests in `workspaces.rs`:
- the URI split keeps raw path and query, and refuses paths outside the API and bad IDs;
- registry least-recently-used eviction and folder layout;
- 401 without a session;
- 403 on writes without the CSRF token or with the wrong one;
- **DB-backed isolation:**
  - the owner can scan and upload;
  - the second project is a separate workspace;
  - user B gets 404 on GET, DELETE and upload;
  - an unknown project is 404;
  - only the owner's two workspaces exist on disk.

Results:
- `check.sh rust archaeodash-api`: fmt, tests and clippy `-D warnings` pass.
- With `DATABASE_URL` pointing at a throwaway `postgres:16-alpine` on home2 (127.0.0.1:55432, removed afterwards), all 61 API tests pass.
- The DB-backed tests skip without `DATABASE_URL`, so plain `check.sh` does not exercise the isolation test. Use the throwaway Postgres or `scripts/verify-local.sh rust`.

## Not done yet (later steps)
- Project or account deletion does not yet remove or close the workspace. Access is already refused, because ownership is checked on every request.
- No workspace quota yet.
- No body limit or job concurrency limits.
- No audit yet of desktop-only routes.
- The web UI does not yet use `/w/<project-id>` (step 2).
- The server does not yet serve `dist/` (step 3).
