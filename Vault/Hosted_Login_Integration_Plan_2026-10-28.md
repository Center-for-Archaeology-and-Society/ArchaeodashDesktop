Up: [[Index]]

# Hosted login integration plan (2026-10-28)

Goal: the server version of ArchaeoDash has user accounts, and each user analyses only their own projects. Planning only; no code changed. It follows [[Cas_Server_Deployment_Recommendation_2026-10-28]].

## What already exists

- **Accounts backend** (`crates/api/src/auth.rs`, `crates/auth`, `crates/control-postgres` migrations 0001–0008). It already has:
  - register, email verification, login, logout, logout-all, session, consent and password reset;
  - an HttpOnly session cookie, with only its SHA-256 digest stored;
  - a double-submit CSRF cookie and the `X-CSRF-Token` header;
  - persistent peppered throttles and enumeration-resistant responses;
  - per-user preferences;
  - a project catalog (`/api/v1/projects`);
  - per-user file upload with quota, hourly retention sweeps, and a transformation catalog.
- **Hosted server** (`src/bin/hosted.rs`). It needs Postgres, `AUTH_PEPPER`, `AUTH_BASE_URL` and an email mode (sendmail, SMTP, or dev only).
- **Web client** (`packages/client/src/http.ts`, `apps/web/src/shell/AccountPage.tsx`, `HostedProjectsPage.tsx`, routes `/account` and `/projects`). It already handles login, register, verify, reset, the account page and the project list. It sends the CSRF header automatically.
- **Analysis API** (`root_router`, `crates/api/src/lib.rs:805`). It is complete, but serves **one** project folder with no auth. Every application service is built from a folder path (`GroupService::new(root)` and the others), using `FsGroupFileStore` and the project lock.
- **Project boundary checks** exist in import, files, clustering and transforms (`canonicalize`, plus "escapes the project boundary" errors).

The missing piece: logging in does not open anything the analysis pages can use. The two routers have never been joined.

## Design choice

**A. Per-project workspace folder (recommended).**
- Each hosted project gets a real folder: `AUTH_FILE_STORE_DIR/users/<user-uuid>/projects/<project-uuid>/workspace/`.
- The existing analysis services run against that folder unchanged, including parquet groups, sub-folders, the project lock and the `.archaeodash/` cache.
- Pros: reuses every analysis feature and the desktop behaviour as they are. Only opaque UUIDs appear on disk.
- Con: group files in the workspace do not go through the Postgres file catalog, so quota means measuring folder size. It is also local-filesystem only (no S3), which is fine for one host such as cas.

**B. Catalog-backed store (the IMPLEMENTATION.md Section 6.4 ideal).**
- Make every application service generic over `GroupFileStore` (`crates/storage/src/lib.rs:465`), and write a store backed by the Postgres catalog and UUID objects (local or S3).
- Pros: S3-ready, and catalog and quota work per file.
- Con: refactors about ten services, and is several times the work of A.

A can later move to B behind the same HTTP surface.

## Work for option A

### Backend (`crates/api`)
1. **Workspace registry** (done 2026-10-28, see [[Hosted_Project_Workspaces_2026-10-28]]). Map `(user_id, project_id)` to an `Arc<AppState>` plus `Router`, built lazily against the workspace folder. Keep it in an LRU cache with idle eviction. Create the folder when the project is created, and remove it when the project is deleted (or move it to trash for the retention sweep).
2. **Project-scoped analysis routes.** Mount the analysis API under `/w/{project_id}/api/v1/...`. A guard layer must:
   - resolve the session (`require_session`), returning 401 when there is none;
   - require CSRF on non-GET requests;
   - check that the user owns the project, returning **404** for someone else's project so it doesn't reveal that the project exists;
   - then pass the request to that project's router.

   A path prefix, rather than a header, keeps SSE job events working, because `EventSource` cannot send headers.
3. **One server binary** (done 2026-10-28, see [[Hosted_Single_Image_2026-10-28]]). `hosted` serves auth, projects, the scoped analysis routes, health checks, and the static `dist/` (with an `index.html` fallback for `/account`, `/projects` and the other routes). Apply the existing security headers and correlation IDs to all of it.
4. **Quota and limits** (done 2026-10-28, see [[Hosted_Multi_User_Hardening_2026-10-28]]):
   - check workspace size against `AUTH_QUOTA_BYTES` before upload or import commit;
   - set a request body limit;
   - limit concurrent analysis jobs per user and overall, since UMAP and clustering are CPU-heavy;
   - keep the existing 100k-row limits.
5. **Desktop-only routes** (audited 2026-10-28; P1 to P3 fixed then, P4 to P6 fixed 2026-10-29). Audit every `root_router` route for paths and features that only make sense on the desktop (absolute paths, log folders, reveal-in-folder). Reject them in hosted mode. Confirm that every path argument goes through the project boundary check, including `groups/{*path}` delete, rename, folders, exports and `source_path`.
6. **Account deletion and retention** (project delete removes the workspace, 2026-10-28; account deletion still open). Deleting an account or project also deletes its workspace folders, and the retention sweep covers them.

### Web UI (`apps/web`)
1. **Hosted startup flow** (done 2026-10-28, see [[Hosted_Web_Login_Flow_2026-10-28]]): no session → Account (login or register); after login → Projects. Choosing a project creates `HttpTransport` with base URL `/w/<project-id>` and opens the Data Manager.
2. **Top bar:** show the signed-in user, the current project, a project switcher and Log out.
3. **Expired sessions:** a 401 from any call goes to the login page and comes back afterwards.
4. **Hide desktop-only controls** (there is already a `desktop` flag in `ShellInfoContext`).
5. **If served under a sub-path:** set Vite `base`, the router `basename`, and API URLs based on `import.meta.env.BASE_URL`.

### Tests
- Isolation:
  - user B gets 404 on user A's project, for every route family, SSE included;
  - CSRF is rejected on analysis writes;
  - path traversal is rejected on every path argument;
  - an expired session returns 401.
- Quota: the limit is enforced.
- Browser e2e: register, verify (dev email sink), log in, create a project, import, run PCA, log out.
- Run these through `scripts/home2/check.sh` and the local Docker verify gate (`deploy/compose/verify.yml`).

### Deployment (cas)
1. **Use a dedicated host name**, for example `archaeodash.rc.asu.edu` (needs ASU DNS plus certbot). With logins, sharing the `cas.rc.asu.edu` origin with Shiny, heurist and CollectiveAccess is a security risk: those apps can read the CSRF cookie and can send requests as the logged-in user, so a cross-site scripting (XSS) bug in any of them reaches ArchaeoDash accounts.
2. **Compose stack:** `postgres:16` with its data on **local disk** (not on the NFS share), plus the app container. Bind both to 127.0.0.1. Keep user workspaces on the NFS share after the project lock test passes. Run nightly `pg_dump` to NFS (`deploy/backup.sh`).
3. **Email:** use SMTP to an ASU relay, or open the host's postfix to the Docker bridge. `AUTH_BASE_URL` must be the public URL, because verification and reset links use it.
4. **Secrets:** `POSTGRES_PASSWORD`, `AUTH_PEPPER` and the SMTP credentials go in a root-only `.env` on cas. Never put them in the repo or the Vault.
5. **Container recipe:** fix `deploy/container/Dockerfile` first (toolchain 1.98.1, workspace members, curl healthcheck, Node stage for `dist/`).

## Decisions for the owner
- Design A (recommended) or B.
- **Who may register:**
  - open registration;
  - ASU email addresses only;
  - invite or admin approval (recommended for the beta).
- Host name (recommended) or a sub-path on `cas.rc.asu.edu`.
- Guest use without an account: the Shiny app allows it. The plan (Section 3.1) wants a short-lived, quota-limited guest space. Recommendation: no guests for the beta.
- Shiny accounts: keep them separate for now. Migration is Phase 8 and needs an authorised decision.

## Suggested order
1. Backend workspace registry, scoped routes and the guard, with the isolation tests.
2. Web hosted flow (login, then projects, then analysis), with the 401 handling.
3. Single binary serving `dist/`, plus the fixed Dockerfile and compose file. Run end to end locally on home2 with the dev email sink.
4. Route audit, quota and job limits.
5. cas: DNS and certificate, compose, Apache vhost, SMTP, backups, healthcheck. Invite-only beta. (2026-10-29: sub-path chosen; repo work and rehearsal done, see [[Cas_Hosted_Deployment_2026-10-29]].)
