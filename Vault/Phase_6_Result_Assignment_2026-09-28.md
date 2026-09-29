# Phase 6 result assignment — 2026-09-28

## Scope

Continue [[Phase_6_Cluster_Plots_2026-09-28]] with hidden UUID identities for cluster, membership, and nearest-match results, UUID-based selection, and confirmed manual moves to existing destination group files. Use the result source revision with the existing journaled transfer transaction. Automatic best/matched-group assignment and recording all clusters into multiple files are separate follow-ups; no calculated elemental values are written.

Three Luna agents own Rust result identity, the pure client selection/request model (medium reasoning for both), and adapter integration tests (low reasoning). The primary agent owns UI integration, TypeScript contracts, review, validation, and incremental commits.

## Progress

Implementation started with a clean worktree. Existing numerical results carry source revisions but lacked stable UUID fields. Existing group transfers provide revision conflicts, immutable-value preservation, and atomic publication; this slice reuses that service.

Related: [[Interaction_Log_2026-09-28]].

## Result identity checkpoint

Cluster and membership responses now carry immutable UUIDs in input row order. Euclidean rows carry both observation and matched UUIDs, while retaining legacy rowid for compatibility. The numerical nearest-match implementation propagates the matched row key without changing distance calculation or ordering. Rust and TypeScript contracts and transport fixtures agree.

Analysis/application/contracts tests and library clippy pass. A regression duplicates both visible IDs and legacy row IDs and still correlates observation/match UUIDs correctly. Initial API/desktop tests verify result-to-transfer identity and stale-source rejection; existing-destination/all-source cases are being added before final integration.
