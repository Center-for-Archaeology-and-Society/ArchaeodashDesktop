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

## Transaction and UI checkpoint

Result selection is UUID-based, including deduplication of repeated nearest-match observations. Hierarchical selection shares the plot cut k while mapping leaves back to input-order UUIDs. Canonical identities and matching vector lengths are required; missing result identities disable assignment. A pure request builder captures the result source path/revision and only allows selected identities present in that result.

The UI offers existing ready destination group files and requires a review followed by Confirm move. UUIDs stay out of labels, checkbox values, and rendered result data. Confirmed requests use the existing move transaction; controls are disabled in flight and a synchronous guard prevents duplicate submissions. Success discards stale analysis results. When all units leave the source, the next dataset is the destination. Refresh failure after a committed move reports success plus the reload error, never retries the transfer.

HTTP/desktop integration tests compare result identities to input rows, move units into new and existing destinations, verify exact measured/visible/descriptive row preservation and source removal, verify advancing revisions, and reject stale retries without changing either file. Desktop coverage also proves the empty-source deletion response and source lookup failure. API tests (17), desktop tests (12), all client workspace tests (69 web), and workspace typechecks pass.

## Final validation and remaining work

- Combined Rust tests pass for analysis, contracts, application, API, desktop, and all existing parity fixtures; numerical results and ordering remain unchanged.
- Client workspace tests (69 web tests), typechecks, production builds, Rust library clippy with warnings denied, Tauri shell compilation, formatting, and diff whitespace checks pass. The existing Plotly bundle-size warning remains.
- Checks are unit/SSR, application, and adapter integration tests. Browser/desktop UI end-to-end acceptance and performance budgets remain open.
- Existing-group manual moves are complete in this increment. Automatic best/matched-group assignment, multi-group cluster recording, PCA inputs, additional cluster metrics/linkages, and cancellable jobs remain open. No migration phase exit is declared from these checks alone.
