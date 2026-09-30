# Phase 6 Final Acceptance Work — 2026-09-30

Continuing the remaining Phase 6 gates with three Luna agents: isolated native
assignment, cancellation/resource-limit audit, and explicit performance-budget
checks. Root reviews integration and the live browser fallback path.

Audit found that the application test named for Hotelling fallback requested
Mahalanobis directly; the browser suite also did not exercise the visible
fallback notice. Genuine missing-value fallback coverage is being added.
The benchmark cancellation flag also previously accepted any error, which is
being corrected to require the cancellation code.

Performance specifications name budgets but provide no portable numeric limits.
Explicit optional limits can be measured and enforced without claiming an
unapproved threshold passed. Native verification is being attempted with
isolated temporary display tools; hosted CI status could not be retrieved for
the local commit (GitHub returned 404).

Related: [[Phase_6_Repeatable_Acceptance_Evidence_2026-09-30]], [[Phase_6_Native_Assignment_Walkthrough_2026-09-30]].

The application fallback test now requests Hotelling on a missing-value fixture,
compares its result to explicit Mahalanobis, and verifies projected-group
metadata. The browser has a twelfth real API-backed case for the same visible
fallback notice, source-row preservation, and hidden UUIDs. All twelve browser
cases passed in 3,608 ms; 95 web tests, the Rust workspace suite, formatting,
and affected-library Clippy passed locally. Cancellation probes now distinguish
the exact cancellation code, work finishing first, and unexpected errors.

Performance runners now accept explicit optional time/memory limits and retain
structured results on failure. Focused tests passed; deliberately tiny budgets
failed as expected (numerical 1.212 s / 19,742,720 bytes; browser 2,931 ms against
1 ms). An unconfigured browser run passed at 2,864 ms with 99,960 points. These
validate enforcement, not approval of a portable budget. Windows peak memory
remains unavailable rather than reporting an incomplete sampled peak.

An isolated native X11 display was made available by extracting temporary test
tools without system installation. Native inspection found assignment controls
underneath the Plotly canvas because WebKit collapsed the holder. An explicit
450-pixel scatter height now reserves layout space; native retesting is underway.

The native retest passed on the rebuilt Linux app. Pointer lasso selected AID669;
the UI committed D1→D2 and cleared selection. Source/destination counts changed
104→103 and 50→51. Read-only comparison of complete Parquet rows against before
snapshots confirmed stable identity, all 33 measured values and descriptive
fields unchanged, and every other source/destination row unchanged. After
close/relaunch and native project reopening, Explore's accessible table showed
103 source rows without AID669 and 51 destination rows with it exactly once.

The new real-pointer browser regression passed: holder height 450px, controls
below the chart, one selected point, full row preserved after assignment, hidden
UUIDs, and durable membership after page reload. It is wired into browser CI.
The final web suite passed 97 tests; analysis/parity with all targets passed 66,
including the example cancellation test. Native production assets and binary
built successfully. Hosted cross-platform results, Windows peak memory, and
ratified portable performance limits remain open; no broader sign-off is claimed.
