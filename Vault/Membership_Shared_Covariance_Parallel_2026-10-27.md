# Membership probabilities: shared group covariance and parallel rows (2026-10-27)

Up: [[Index]] · [[ArchaeoDash_Analysis_MOC]]

## Why
- `group.mem.probs` (Hotellings and Mahalanobis) ran on one core. For every (row, group) pair it scanned all rows to build the group and recomputed that group's mean, covariance, and inverse from scratch, even though every row outside a group sees the same full group.

## What (`crates/analysis/src/membership.rs`)
- Each test is now split into a group step (mean, covariance, inverse) and a row step (quadratic form, p-value).
- Rows outside a group reuse one shared group step. For Mahalanobis the step is keyed by (group, the row's finite-column pattern), because `getMahalanobis` filters columns and cases by the row's finite cells. Only pairs some outside row uses are prepared, so work never exceeds the old per-pair loop.
- Rows inside a group still get their own leave-one-out step, computed exactly as before. No rank-one downdate, because that would change the floating-point results.
- Group members are precomputed once (ascending index, so R's `which()` order is kept). Rows run in parallel with `pairwise::par_row_blocks` (16-row blocks), and setup tasks run in parallel too. Cells are assembled in row order, so results don't depend on scheduling.
- Error semantics are unchanged. A singular whole-group solve only matters when an outside row uses it, and any Hotellings pair failure still falls back to the whole-table Mahalanobis path. Cancellation now stops all workers; `mahalanobis_cells` returns `Result`.

## UMAP faer toggle (`crates/analysis/src/umap.rs`)
- `smallest_eigenpairs` used to set faer's process-wide parallelism to `Seq` and then restore it. Two overlapping UMAP jobs could race: one solver could run multi-threaded and lose bit determinism, or the global setting could be left at `Seq`.
- It now calls `faer::linalg::evd::self_adjoint_evd` directly with `Par::Seq`. That is the same routine `SelfAdjointEigen::new` calls, so the results are the same and no global state is touched.

## Verification
- New tests compare the shared/parallel tables against the original per-pair loop bit for bit. The data has 150 interleaved rows in 4 groups, one of them ineligible, plus NaN/Inf cells, an all-NaN row, a constant column, and the singular Hotellings fallback. A cancellation test is included.
- `check.sh rust archaeodash-analysis` (62 tests), `check.sh rust archaeodash-application` (117 tests), and `check.sh parity` (R golden fixtures, including membership and UMAP) all pass; fmt and clippy are clean.
- No speed benchmark was run. The expected gain is about (number of eligible groups) × (cores) for the outside-row work.
- Committed (no push); the Windows exe has not been rebuilt yet.

Related: [[Interaction_Log_2026-10-27]]
