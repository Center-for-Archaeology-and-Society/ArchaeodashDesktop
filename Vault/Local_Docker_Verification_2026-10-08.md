# Local Docker Verification Replaces Hosted CI (2026-10-08)

GitHub Actions on this account is blocked by the billing/spending-limit
failure recorded in [[Phase_7_Threat_Model_Closure_2026-10-03]]: every
workflow run dies before any step. At owner request, the hosted `ci.yml`
workflow was removed and its Linux gates replaced by a local Docker
verification that does not depend on Actions at all.

## What was built

- **`deploy/compose/verify.yml`** — three services:
  - `postgres`: `postgres:16-alpine`, `control_test` database, healthcheck,
    no host port publishing (only the check services reach it, so the gate
    never collides with a developer's local PostgreSQL).
  - `rust-check`: built from `deploy/verify/rust.Dockerfile`
    (`rust:1-bookworm` + the Tauri system libraries CI used to apt-install:
    webkit2gtk-4.1, gtk-3, ayatana-appindicator, librsvg, pkg-config; the
    pinned 1.98.1 toolchain with rustfmt/clippy preinstalled). The repo is
    bind-mounted at `/workspace` with `CARGO_TARGET_DIR=/target` on a named
    volume, so repeated runs reuse the container-side build cache and never
    clobber the host `target/`.
  - `node-check`: `node:22-bookworm`, corepack pnpm (version from
    `packageManager`), pnpm store on a named volume.
- **`scripts/verify-local.sh`** — runs `postgres up --wait`, then, in
  order: `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D
  warnings`, `DATABASE_URL=postgres://postgres:test@postgres:5432/control_test
  cargo test --workspace`, and `pnpm install --frozen-lockfile && pnpm -r
  build && pnpm -r test`. Fails on the first non-zero exit; accepts an
  optional job filter (`rust` or `node`); tears the compose project down on
  exit.

## Equivalence with the removed `ci.yml`

| ci.yml job | verify-local equivalent |
|---|---|
| `rust` (ubuntu-latest + postgres service): fmt, clippy `-D warnings`, `cargo test --workspace` | `rust-check` service, same commands, same `DATABASE_URL` service pattern |
| `node` (ubuntu-latest): pnpm install/build/test | `node-check` service, same commands |
| `numerical-parity`, `filesystem-lock-process` (three OS matrix) | partially covered: the Linux legs run inside `cargo test --workspace`; the macOS/Windows legs remain hosted-CI-only |
| `desktop-cross` (macOS/Windows), `desktop-launch-smoke` (macOS/Windows) | no local equivalent — these stay gated on Actions billing |

The Phase 6 workflows (`phase6-browser.yml`, `phase6-performance.yml`) and
the legacy `r-tests.yml` remain defined; the browser and performance
workflows are equally blocked by the billing failure until the owner
resolves it or chooses to move them local too.

## Verification

- Full first run (image build + cold container compile of the workspace
  including the Tauri crates, live-Postgres test run, pnpm build/tests)
  passed end to end with `verify-local: all gates passed`.
- The DB-backed suites (auth lifecycle, control-plane store tests including
  the transformation catalog, hosted route tests) exercise real PostgreSQL
  16 inside the compose network, matching CI's service-container pattern.
- One environment-dependent test surfaced by the container (root user):
  `missing_log_dir_degrades_to_stdout_without_panicking` used
  `/nonexistent/xyz/logs`, which root can actually create, so the file
  appender started and the degrade-to-stdout assertion failed. The test now
  blocks the directory with a regular file under the temp dir
  (ENOTDIR for any user), making the property environment-independent.

## Related

- [[Phase_7_Threat_Model_Closure_2026-10-03]]
- [[Phase_6_Repeatable_Acceptance_Evidence_2026-09-30]]
- [[Phase_6_Hosted_Cross_Platform_Evidence_2026-09-30]]
