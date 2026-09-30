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
