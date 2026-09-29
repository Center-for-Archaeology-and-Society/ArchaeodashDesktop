# Phase 6 automatic assignment — 2026-09-28

## Scope

Continue [[Phase_6_Result_Assignment_2026-09-28]] with best-group, nearest-matched-group, and cluster-cut assignment plans. Users review an explicit group-label-to-file mapping before confirming a single batch transaction. Cluster recording creates group files from original measured values; it does not persist calculated elemental columns.

Three Luna agents own the batch transaction core/storage safeguards, adapters/transports, and pure recommendation planner; the primary agent integrates and verifies the UI. Medium reasoning is used for transaction and recommendation logic, low for adapter wiring.

## Findings and progress

Existing journal execution did not protect a newly planned destination from a file appearing before publication, and partial rename failure discarded the journal. The batch slice will address these storage gaps before claiming atomic assignment. Phase exit acceptance and external-writer concurrency limitations must be reported accurately.

Related: [[Interaction_Log_2026-09-28]].
