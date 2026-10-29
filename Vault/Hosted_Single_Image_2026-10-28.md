Up: [[Index]]

# Hosted server as one image: API plus web client (2026-10-28)

Step 3 of [[Hosted_Login_Integration_Plan_2026-10-28]]. It follows [[Hosted_Project_Workspaces_2026-10-28]] and [[Hosted_Web_Login_Flow_2026-10-28]]. Not committed.

## What changed
- **`crates/api/src/hosted.rs`:**
  - `serve_web_ui(router, dist)` serves the built web client for every path no API route claims: files from `dist`, and `index.html` for app routes (`/explore`, `/account`, …). Unknown `/api/...`, `/w/...` and `/healthz` paths stay JSON 404s. It is applied before `finalize_hosted_router`, so static files get the security headers too.
  - `/healthz` (app name and version) is added to `hosted_router`, so the sidebar footer and Home show the version instead of "API unavailable".
- **`crates/api/src/bin/hosted.rs`:** new env `AUTH_WEB_DIR`. When set, it must contain `index.html` (startup fails otherwise) and the server serves the web client from it. Unset means API only.
- **`deploy/container/Dockerfile`** (rewritten):
  - `web` stage: `node:22-bookworm`, corepack pnpm install `--frozen-lockfile`, then `pnpm --filter ./apps/web build`. It needs `tsconfig.base.json`.
  - `api` stage: `rust:1-bookworm`, toolchain 1.98.1 (same pin as `rust-toolchain.toml`), `cargo build --release --locked -p archaeodash-api --bin hosted`, with BuildKit cache mounts for the registry and target. All workspace member manifests are copied (`crates/`, `apps/desktop/src-tauri`, `tests/parity`).
  - runtime stage: `debian:bookworm-slim` with curl, uid 10001, `/data/user-files` owned by 10001 (a named volume inherits this), the web client at `/srv/archaeodash/web`, a `HEALTHCHECK`.
  - Default env: `AUTH_BIND=0.0.0.0:8080`, `AUTH_WEB_DIR`, `AUTH_FILE_STORE_DIR`, `AUTH_LOG_FORMAT=json`.
  - Old problems fixed: Rust 1.82 vs 1.98.1, missing workspace members, a wget healthcheck in an image without wget, and no web client.
- **`deploy/container/Dockerfile.dockerignore`** (new): an allowlist build context (about 6 MB). The repository `.dockerignore` belongs to the R image and is left alone.
- **`deploy/container/healthcheck`:** now uses curl, takes the port from `AUTH_BIND`, and checks `/api/v1/health/live` then `/ready`.
- **`deploy/compose/hosted-stack.yml`:**
  - build context fixed from `..` to `../..`; the old value pointed at `deploy/`, so the build could never find the Dockerfile;
  - `AUTH_OPEN_WORKSPACES` passthrough;
  - a note that the image includes the web client.

## Verification
- `check.sh rust archaeodash-api`: fmt, 63 tests (2 new: `/healthz` on the hosted router; `serve_web_ui` assets, app-route fallback, JSON 404s, API routes win, security headers on static files) and clippy pass.
- **Image build on home2:** `docker build -f deploy/container/Dockerfile -t archaeodash-hosted:dev .`. The web stage takes 27 s, the Rust release (thin LTO, `codegen-units=1`) about 9 min cold, and the image is **137 MB**. The tag `archaeodash-hosted:dev` is kept on home2.
- **Container run:** a Docker network with a throwaway `postgres:16-alpine`, a named volume, `127.0.0.1:18080`, dev email.
  - `/`, `/explore`, `/account` return `index.html`; assets 200; `/healthz` 200 with the version; `/api/v1/nope` 404 JSON; `/w/<id>/...` 401 without a session.
  - Docker health "healthy"; volume owned by 10001.
- **Seeding** with curl through the public API only: register, verify (link from the dev-sink log), log in, create a project, upload `fixtures/INAA_test.csv` to `/w/<id>/api/v1/files`, import preview and commit grouped by `Cer_grp`. Result: 307 rows in 16 group files.
- **Browser check** (tunnel to `localhost:18080`):
  - sign in → Projects → Open "INAA demo";
  - Data Manager lists all 16 groups, and Explore shows 307 rows;
  - a Plotly histogram and PCA (explained-variance chart and table, PC1 81.7%) render;
  - **no CSP violations** under the API's strict CSP (`script-src 'self'`), and no 5xx responses in the server log.
  - Everything was removed afterwards, apart from the image.

## Known gaps
- Cosmetic: before sign-in, the app requests `GET /api/v1/preferences` (401, theme load) and `GET /api/v1/transformations` (404, unscoped because no project is open yet). Both are harmless console noise. The second could be skipped until a project is open.
- The release build is slow cold (about 9 min); the BuildKit cache mounts help rebuilds.
- Plan steps 4 and 5 remain: route audit, workspace quota, body and job limits, deleting a project's workspace; then the cas deployment (host name, Apache vhost, SMTP, backups).
