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

## Client workflow checkpoint

Result selection now offers manual assignment or analysis-group assignment. Best/matched group labels and cluster-cut labels receive an explicit destination mapping, with existing groups, new group files, and Keep in current group options. A unique exact group-name match may prefill a choice; ambiguous names remain unselected. The review table shows outgoing group paths and counts plus retained-source counts before confirmation. Source and destination revisions are carried in the batch request; changing the cut is disabled during review.

The batch refresh workflow calls the batch endpoint once and never loops individual transfers. A committed batch followed by a failed refresh is reported as committed rather than retried. Empty-source batches reopen the first destination. Recommendations and UI errors do not display hidden UUIDs. Client workspace tests (79 web tests), typechecks, and production build pass; the existing Plotly bundle warning remains. Storage hardening validation is pending final review.

## Transaction integration and validation

The shared batch service validates all UUID selections, source/destination revisions, destination schemas, and file aliases before executing one journaled MoveUnits transaction. Existing and new destination files retain original measured and descriptive row values; an emptied source is removed. HTTP and desktop adapters expose the same service.

Filesystem execution and recovery hold a per-project advisory lock. New outputs publish through no-clobber hard links, partial publication rolls back, and failed rollback retains its journal. Recovery checks retained staged-file ownership before removing a created output, preserving ambiguous files and journals. Post-commit cleanup failure does not report a failed transaction that could invite retry.

Application (58), contracts (11), filesystem-store (11), API (18), desktop (13), and storage (5) tests pass. Coverage includes stale final destinations, incompatible schemas, duplicate assignments, hard-link aliases, partial publication, and destination creation races. Rust library clippy with warnings denied, workspace formatting, Tauri compilation, and diff checks pass. Client workspace tests include 79 web tests; typechecks and production builds pass.

## Remaining acceptance limits

The project lock coordinates app transactions; unrelated external filesystem writers do not honor it. Preflight reads and multi-file readers are not snapshot-isolated. Ambiguous legacy creation journals without retained staged ownership evidence fail closed and remain for operator recovery. This increment does not establish full filesystem concurrency or cross-platform crash acceptance.

Browser/desktop UI end-to-end acceptance and performance budgets remain open; the build retains its existing Plotly size warning. Phase 6 still needs PCA input controls, additional distance/linkage options, cancellable jobs, and full statistical/operational exit acceptance.
