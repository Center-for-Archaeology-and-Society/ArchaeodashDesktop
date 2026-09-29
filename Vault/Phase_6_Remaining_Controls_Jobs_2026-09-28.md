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
