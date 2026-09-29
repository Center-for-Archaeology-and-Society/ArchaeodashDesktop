# Phase 6 remaining controls and jobs — 2026-09-28

## Scope

Continue [[Phase_6_Automatic_Assignment_2026-09-28]] with the remaining statistical sources, distance/linkage controls, and cancellable analysis jobs. Three Luna agents implement numerical methods, shared input preparation/contracts, and the job lifecycle independently; the primary agent integrates transports and client controls. Phase exit requires evidence, not implementation alone.

## Initial audit

The current service supports elements and transformed cluster matrices, Euclidean PAM/DIANA and Ward.D2, and synchronous bounded analysis calls. Required remaining controls include PCA components, membership/matching ordination sources and projection groups, hierarchical metric/linkage choices, PAM diagnostics, and real cooperative cancellation with a bounded job queue. Existing pairwise limits are conservative guards, not performance acceptance.

Related: [[Interaction_Log_2026-09-28]].

## Client checkpoint

Client requests now expose measured/transformed columns, ephemeral PCA/UMAP/LDA sources, component counts, projection groups, metric/linkage controls, k-means starts/iterations, and k-means/PAM diagnostics. Typed HTTP/Tauri job transports share a submit-once polling workflow. Cancellation during submission, worker acknowledgement, deadline errors, and orphan cancellation after transport failures have focused tests. Client workspace tests and typechecks pass (86 web tests). Browser interaction checks against a local API harness are next.

## Numerical and job checkpoint

Generic HCA now follows R's nearest-neighbor agglomeration and merge recoding, including exact tie orientation and leaf order. R probes cover Average, Complete, Ward.D, Ward.D2, Manhattan, Maximum, and nondefault Minkowski power; Euclidean legacy goldens remain intact. PAM and DIANA accept Manhattan distance. Cooperative tokens reach cluster starts/iterations, membership/matching loops, and UMAP neighbors/epochs. Exact nearest matching retains only the requested top candidates, preserving NaN/tie order and the legacy post-limit same-group filter.

The project-local job pool admits at most two workers and sixteen queued operations, retains at most 32 snapshots, caps each result at 8 MiB, defaults to a ten-minute deadline, and rejects deadlines above thirty minutes. Dropping the last pool owner requests cancellation. Lifecycle, queue, cancellation, timeout, numerical, and production clippy checks pass. Hosted per-user quotas remain a Phase 7 concern.

A real Chromium browser run against Vite and the Rust loopback API passed PCA/HCA Manhattan Average, cut/expanded dendrogram controls, PAM Manhattan diagnostics, UMAP cancellation, unchanged source rows, and hidden UUID checks. Further membership, recording, and source checks are in progress.

## Integrated controls, plots, and resource guards

All four analysis operations accept ephemeral PCA/UMAP/LDA sources, transformations, and source controls. Membership exposes requested/effective method, fallback reason, and projection inclusion; matching accepts projection candidates. PAM diagnostics use factoextra's pairwise within-cluster sum of squares. Partition plots provide cluster colors on standardized coordinates and existing-group colors on the original selected dimensions, with explicit projection fallback warnings. Legacy Ward.D2 rejects contradictory metadata options.

A footer-only file preflight checks actual Parquet rows, full-schema cells, and encoded/decoded byte limits before loading analytical data. Source preparation also bounds transformation expansion, UMAP pairwise allocation, and ordination feature counts. Eigenvalue decompositions remain non-preemptible inside their bounded linear-algebra calls; cancellation checks occur before and after them.

The expanded browser run passed ten cases in 3.26 seconds locally, including LDA membership, UMAP matches, projection groups, partition color controls, and confirmed two-group recording. Recorded rows exactly matched the original UUID/visible/descriptive/measured rows. Rust adapter/application/parity suites, 87 web tests, typechecks, and the production build pass. HTTP SSE and Tauri progress watchers are bounded and expose lifecycle updates; client polling remains the authoritative recovery path.

Benchmark and parity evidence is recorded in [Phase 6 validation](../docs/operations/phase-6-validation-2026-09-28.md). It distinguishes local measured timings from unaccepted cross-platform/desktop UI performance gates.

## Full local matrix and browser verification

The checked-in supplementary R fixture covers all sixteen HCA metric/linkage combinations on tied and non-tied matrices, with exact merge orientation/order and tolerant heights; both PAM and DIANA metrics are also covered. All 26 parity/support tests pass. The separately required two-mean Mahalanobis utility is ported and tested. Silhouette computation now checks cancellation, and the legacy Ward.D2 metadata path rejects inconsistent options.

The final source/plot/browser pass completed ten real API-backed cases in 3.31 seconds; the partition SVG was also inspected visually. Progress subscriptions have cleanup tests for late Tauri listener/watch completion. Review found that the desktop shell had no project-opening command despite adapter support; native folder selection is being wired so Phase 6 is usable from the desktop shell before declaring implementation finished.

## Final implementation checkpoint

Three Luna agents completed the remaining source, numerical, and job work with root integration. Native project selection is now implemented; see [[Phase_6_Desktop_Project_Selection_2026-09-28]]. Direct procedure 13 comparison corrected sampling to legacy per-group/facet source order (99,960 exact selected points), and visual input arrays now correctly transpose API rows into plot columns. Component bounds, job reset cleanup, and the missing app stylesheet reference were fixed.

Final validation: Rust workspace tests, 92 web tests, 38 client tests, workspace typechecks/build, Rust formatting, and affected-library Clippy (including Tauri) pass. The production-built web app passed eleven real API-backed Chromium cases in 3.375 seconds; source row equality survived reviewed two-group recording. The reusable browser script and pinned Playwright dependency are checked in. No new notes are orphaned.

Phase 6 is not signed off: broader native workflow/assignment acceptance, cross-platform timing/peak memory, and a portable browser performance budget remain open. The actual 100,000-point browser render has now been exercised twice locally. Shared project locks now coordinate group reads, multi-group planning snapshots, candidate scans, import publication, and store transactions among cooperating processes. External programs that ignore the advisory lock remain outside that boundary. See [[Filesystem_Group_Read_Locks_2026-09-29]], [[Filesystem_Group_Read_Snapshots_2026-09-29]], [[Filesystem_Project_Scan_Lock_2026-09-29]], and [[Filesystem_Import_Lock_Coordination_2026-09-29]].
