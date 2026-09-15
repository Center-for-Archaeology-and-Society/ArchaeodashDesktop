# Phase 1 Monorepo Skeleton 2026-09-14

Up: [[../IMPLEMENTATION]]

## Outcome

Phase 1 (monorepo and domain skeleton) exit criteria from [[../IMPLEMENTATION]] Section 16 are met and verified:

- Rust workspace at repo root with all 15 crates from Section 4 (`domain`, `contracts`, `analysis`, `data-io`, `storage`, `file-store-fs`, `file-store-s3`, `project-manifest`, `cache`, `jobs`, `auth`, `control-postgres`, `application`, `api`, `desktop`) plus the `apps/desktop/src-tauri` Tauri 2 shell.
- Toolchain pinned: `rust-toolchain.toml` (1.98.1 + rustfmt/clippy), `rustfmt.toml`, workspace lints (`unsafe_code = forbid`, clippy `unwrap_used`/`expect_used` warned).
- Domain crate implements Section 5.1 opaque IDs (`UserId`, `SessionId`, `ProjectId`, `GroupId`, `GroupRevisionId`, `TransformationId`, `AnalysisResultId`, `JobId`), UUIDv7 `AnalyticalUuid`, `SourceRef`, and typed `DomainError`.
- Contracts crate holds the transport-neutral DTOs (`AppInfo`, `ErrorEnvelope`, `JobState`) shared by HTTP and Tauri.
- Smoke use case (`app_info`) executes through both adapters: Axum `/healthz` route test and Tauri `#[tauri::command]` delegation test — the Phase 1 exit requirement.
- pnpm/TS workspace: `packages/contracts`, `design-system` (three theme tokens), `test-fixtures`, `client` (Transport abstraction + `HttpTransport`), `apps/web` (Vite + dev proxy to Axum on 8787). Node 22, pnpm 12.4.1 via corepack.
- CI workflow `.github/workflows/ci.yml` runs cargo fmt/clippy/test and pnpm build/test.
- Section 4 skeleton directories created: `migrations/{postgres,project-manifest}`, `deploy/{container,compose,systemd}`, `tests/{parity,contract,integration,e2e-web,e2e-desktop,security,performance}`.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`: clean.
- `cargo test --workspace`: 18 passed, 0 failed.
- `pnpm -r build` (5 packages + web Vite bundle) and `pnpm -r test`: all green.
- `Cargo.lock` and `pnpm-lock.yaml` generated for commit.

## Decisions

- Numerical backend deferred to the analysis crate (Phase 3/4); `faer 0.24` pinned in workspace deps per Section 17.1 default.
- Node strip-only test runner avoids a heavy test framework until Playwright/vitest land in Phase 5.
- Tauri capability grants stay `core:default` only; filesystem scopes arrive with Phase 2 desktop commands.

## Environment notes

- WSL is the build environment; invoke via script files under `tools/` because PowerShell mangles `$` in inline `wsl.exe` commands.
- `tools/dev-env-check.sh`, `cargo-lint.sh`, `cargo-test.sh`, `pnpm-build-test.sh` remain as reusable verification entry points.

## Related

- [[R_Oracle_Baseline_Capture_2026-09-14]] (Phase 0 goldens these will be tested against)
- [[Implementation_Readiness_Audit_2026-09-14]]
