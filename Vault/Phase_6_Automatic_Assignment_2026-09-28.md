# Phase 6 automatic assignment — 2026-09-28

## Scope

Continue [[Phase_6_Result_Assignment_2026-09-28]] with best-group, nearest-matched-group, and cluster-cut assignment plans. Users review an explicit group-label-to-file mapping before confirming a single batch transaction. Cluster recording creates group files from original measured values; it does not persist calculated elemental columns.

Three Luna agents own the batch transaction core/storage safeguards, adapters/transports, and pure recommendation planner; the primary agent integrates and verifies the UI. Medium reasoning is used for transaction and recommendation logic, low for adapter wiring.

## Findings and progress

Existing journal execution did not protect a newly planned destination from a file appearing before publication, and partial rename failure discarded the journal. The batch slice will address these storage gaps before claiming atomic assignment. Phase exit acceptance and external-writer concurrency limitations must be reported accurately.

Related: [[Interaction_Log_2026-09-28]].

## Recommendation and transport checkpoint

The recommendation planner maps selected UUIDs to fit/cut clusters, eligible finite best-group results, or nearest finite matched groups. Cross-group equal-distance ties require manual assignment; missing recommendations fail closed. Group labels are mapped explicitly to destination files with reviewed source/destination revisions, optional new group creation, and Keep in current group. Missing mappings and colliding paths are rejected; normal error messages omit internal UUIDs.

HTTP, desktop, and TypeScript batch surfaces are wired to one batch service. Focused adapter tests and the shared client transport suite pass. Storage publication/recovery hardening is still under review before final integration.
