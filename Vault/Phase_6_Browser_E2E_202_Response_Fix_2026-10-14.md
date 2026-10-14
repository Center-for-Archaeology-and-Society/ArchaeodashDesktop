# Phase 6 Browser E2E 202 Response Fix (2026-10-14)

## Symptom

Every hosted `Phase 6 browser E2E` run from 2026-10-08 onward failed in the
"Real API and production preview" job at `scripts/e2e/phase6.mjs:38` —
`locator.waitFor: Timeout 30000ms exceeded` for the `Analysis results` region.
The page dump's last line rendered `TypeError: Cannot read properties of
undefined (reading 'state')`: the app itself displayed the error banner, so no
results region ever appeared.

## Root cause

Commit 2dabc71 (hosted auth transport, Section 9.2/10.1) changed
`HttpTransport.unwrap` in `packages/client/src/http.ts` to return `undefined`
for **both 204 and 202** responses. The auth routes return a bare `202
Accepted` with no body, but `POST /api/v1/jobs` and `POST
/api/v1/jobs/{id}/cancel` return **202 with a JSON job snapshot**
(`crates/api/src/lib.rs` `jobs_submit`/`jobs_cancel`). The analysis job
submission therefore resolved to `undefined`, and
`apps/web/src/clustering/job-workflow.ts:35` threw reading `job.state`.
Desktop/Tauri was unaffected (no HTTP unwrap), and the auth-security browser
e2e never submits analyses, so it kept passing.

The regression window was hidden by the GitHub Actions billing/spending-limit
block: the last green run was 2da98207 (2026-10-02); runs from 2026-10-02
09:20 through 2026-10-05 died at startup in 3–7 s ("recent account payments
have failed or your spending limit needs to be increased"), and the first
post-block run (2026-10-08) executed the test against the already-broken code.

## Fix

`unwrap` now treats 204 as body-less, and 202 as body-less only when the body
is empty; otherwise it parses the JSON. Added a client contract test
(`202 with a JSON body parses the job snapshot; 202 without a body is
undefined`) that fails against the old code.

Also: `crates/api/examples/local.rs` now honors `LOCAL_API_ADDR` (default
unchanged, `127.0.0.1:8787`) so local e2e runs can dodge port collisions such
as the gluetun Docker container already bound to 8787 on the dev machine.

## Verification (local, real API + production preview)

- `phase6.mjs`: passed, all 12 cases, 4.9 s.
- `visualize-assignment.mjs`: passed (real-pointer lasso, row preservation,
  reload persistence).
- Client package tests: 46/46 (node 24 native type stripping; the workspace
  `pnpm test` loader path needs `amaro`, unavailable on this host's
  partially read-only node_modules mount).
- `cargo fmt --check` and `cargo clippy -p archaeodash-api --example local
  -- -D warnings` clean.

## Diagnostics notes

- Actions cache deletion needed the CAS fine-grained PAT (Actions read/write);
  the personal PAT was effectively read-only on the org repo. ~10.94 GiB of
  rebuildable caches were deleted on 2026-10-14.
- Billing-blocked runs can be distinguished from real failures by duration:
  3–7 s failures are startup blocks, not test failures.
- `gh run download -n <artifact>` retrieves phase6.log/api.log/web.log for
  post-mortem without re-running.
