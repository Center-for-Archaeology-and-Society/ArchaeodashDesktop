# Phase 6 Repeatable Acceptance Evidence — 2026-09-30

The next acceptance step uses three Luna agents for bounded work: cross-platform
numerical CI, numerical timing/peak-memory capture, and real API-backed browser CI.
The primary agent reviews integration and records local verification here.

These checks collect evidence for the existing Phase 6 gates. They do not close
native WebView assignment acceptance, approve a portable performance budget, or
advance hosted authentication/cutover.

Related: [[Phase_6_Remaining_Controls_Jobs_2026-09-28]], [[Phase_6_Native_Assignment_Walkthrough_2026-09-30]].

The numerical CI matrix runs the analysis and R-golden parity crates on Ubuntu,
macOS, and Windows without Tauri dependencies. Local analysis (39 tests) and
parity suites passed; the workflow parses as YAML. Hosted runs remain pending.
