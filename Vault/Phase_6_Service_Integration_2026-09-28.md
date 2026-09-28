# Phase 6 service integration — 2026-09-28

## Checkpoint

The plan header was stale. Code and linked phase notes show implementation slices through Phase 5 and numerical goldens for Phase 6. Phase 5 desktop assignment e2e and performance acceptance are still open. Phase 6 UI routes are placeholders; full statistical matrix approval and job cancellation remain open.

Existing working-tree edits include Phase 5 interactive multiplots and Phase 6 Rust service/contracts/adapters. These were preserved. Three Luna agents own separate client transport, service hardening, and adapter-test tasks; service hardening uses medium reasoning, mechanical integration tasks use low reasoning. The primary agent integrates and verifies the results.

## Validation and remaining work

Integration in progress. The configured pnpm shim points at a missing `.cjs` entry; running the installed `bin/pnpm.mjs` succeeds. Baseline client tests passed, and the existing Phase 5 interactive multiplot work was committed as `18a51ed`.

The typed Phase 6 client transport now implements the four shared operations through HTTP and Tauri; contracts/client tests and typechecks pass. Adapter integration tests pass against imported/merged group fixtures. Tauri shell imports/state were repaired and `cargo check -p archaeodash-desktop-app` passes. Service review added project containment and bounded pairwise resources; numerical edge-case review is ongoing before committing that increment.

Related: [[Phase_6_Membership_Euclidean_Goldens_2026-09-24]], [[Phase_5_Interactive_Multiplot_Plotly_2026-09-24]], [[Interaction_Log_2026-09-28]].

## Service and numerical verification

The Phase 6 service resolves canonical project-contained paths, rejects escaping symlinks, caps pairwise matrices at 1,000,000 cells (1,000 rows) and input matrices at 4,000,000 cells, bounds k-means iterations/starts, and validates seeds without truncation. Euclidean self-exclusion uses immutable UUIDs internally, preserving distinct observations even with duplicate legacy row IDs. Result payloads retain the source revision and membership effective method. HTTP CPU work runs on the blocking pool.

Review corrected singleton silhouette widths to zero, restored undefined trivial-cluster means, and made fallback best-group selection follow the effective Mahalanobis method (minimum distance). Regression tests and numerical goldens 09–11 pass. The membership service fixture previously had singular covariance; a nonsingular deterministic fixture now exercises the intended Hotelling path. Analysis unit tests (30), application tests (52), and API tests (17) pass.

These resource limits are conservative service limits, not measured performance acceptance. Input-file reading and transformation expansion can allocate before the service checks dimensions. Jobs, cancellation, and full performance acceptance remain future work.
