# Phase 6 service integration — 2026-09-28

## Checkpoint

The plan header was stale. Code and linked phase notes show implementation slices through Phase 5 and numerical goldens for Phase 6. Phase 5 desktop assignment e2e and performance acceptance are still open. Phase 6 UI routes were placeholders at the start of this interaction; full statistical matrix approval and job cancellation remain open.

Existing working-tree edits include Phase 5 interactive multiplots and Phase 6 Rust service/contracts/adapters. These were preserved. Three Luna agents own separate client transport, service hardening, and adapter-test tasks; service hardening uses medium reasoning, mechanical integration tasks use low reasoning. The primary agent integrates and verifies the results.

## Validation and remaining work

Integration in progress. The configured pnpm shim points at a missing `.cjs` entry; running the installed `bin/pnpm.mjs` succeeds. Baseline client tests passed, and the existing Phase 5 interactive multiplot work was committed as `18a51ed`.

The typed Phase 6 client transport now implements the four shared operations through HTTP and Tauri; contracts/client tests and typechecks pass. Adapter integration tests pass against imported/merged group fixtures. Tauri shell imports/state were repaired and `cargo check -p archaeodash-desktop-app` passes. Service review added project containment and bounded pairwise resources; numerical edge-case review is ongoing before committing that increment.

Related: [[Phase_6_Membership_Euclidean_Goldens_2026-09-24]], [[Phase_5_Interactive_Multiplot_Plotly_2026-09-24]], [[Interaction_Log_2026-09-28]].

## Service and numerical verification

The Phase 6 service resolves canonical project-contained paths, rejects escaping symlinks, caps pairwise matrices at 1,000,000 cells (1,000 rows) and input matrices at 4,000,000 cells, bounds k-means iterations/starts, and validates seeds without truncation. Euclidean self-exclusion uses immutable UUIDs internally, preserving distinct observations even with duplicate legacy row IDs. Result payloads retain the source revision and membership effective method. HTTP CPU work runs on the blocking pool.

Review corrected singleton silhouette widths to zero, restored undefined trivial-cluster means, and made fallback best-group selection follow the effective Mahalanobis method (minimum distance). Regression tests and numerical goldens 09–11 pass. The membership service fixture previously had singular covariance; a nonsingular deterministic fixture now exercises the intended Hotelling path. Analysis unit tests (30), application tests (52), and API tests (17) pass.

These resource limits are conservative service limits, not measured performance acceptance. Input-file reading and transformation expansion can allocate before the service checks dimensions. Jobs, cancellation, and full performance acceptance remain future work.

## Client workflow increment

The Cluster, Probabilities and Distances, and Euclidean routes now use the shared typed transport. Controls select group files and measured columns, algorithms, group/ID columns, seeds, and match limits. Results show diagnostics, partitions, merge heights, effective membership method, and nearest matches. Tables initially render 100 rows with an explicit continuation control. Internal UUID/row keys are excluded from the displayed result tables. Changing datasets/settings invalidates pending results. The obsolete shell placeholders were removed.

Client typechecks and tests pass, including result visibility, fallback labels, merge/diagnostic rendering, and initial table bounds. Production build passes with the existing large Plotly chunk warning. These are unit/SSR and adapter tests, not browser or desktop end-to-end acceptance.

## Remaining phase gates

- Phase 5: desktop project-opening command/UI wiring, web/desktop assignment e2e, and plot performance acceptance.
- Phase 6: additional distance/linkage options; PCA/PC-count/source controls; automatic best/matched-group assignment and multi-group cluster recording; jobs/progress/cancellation; performance budgets and statistical matrix approval.
- Service caps do not replace job cancellation or dataset benchmark acceptance. The current pairwise ceiling is 1,000 rows.
- Hosted authentication and operations (Phase 7) have not been declared complete or started as a substitute for these gates.

## Final validation checkpoint

- Combined Rust suite passed for analysis, contracts, application, API, desktop, and all existing parity tests (including PCA/UMAP/LDA and goldens 09–11).
- Final nullable-distance regression keeps missing-data Euclidean results valid JSON (`null`), round-trippable by Rust, and correctly typed in TypeScript. Application tests now total 53; contracts tests total 11.
- All client workspace tests/typechecks and production builds pass (42 web tests). The build retains the Plotly bundle-size warning.
- Final API/desktop tests, `cargo fmt --all -- --check`, Tauri shell compilation, and library clippy with warnings denied pass.
- New Vault notes have incoming links from the index/interaction log; no new orphan note was introduced.
- Commits during this interaction separately capture the status checkpoint, existing multiplot completion, typed transports, Rust services/adapters, client pages, and final wire-contract cleanup.

## Follow-up

Diagnostic charts and horizontal dendrograms with cut coloring, leaf-size controls, and expanded viewports are implemented in [[Phase_6_Cluster_Plots_2026-09-28]].

Manual UUID-based result selection and assignment are implemented in [[Phase_6_Result_Assignment_2026-09-28]].
