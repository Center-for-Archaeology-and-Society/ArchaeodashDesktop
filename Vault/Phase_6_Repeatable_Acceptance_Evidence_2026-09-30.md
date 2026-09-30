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

A manual performance matrix captures release-example stdout, elapsed time,
process peak RSS on Linux/macOS, and explicit unavailability on Windows. Build
time is excluded. Captures do not establish per-algorithm or native-app memory
budgets. Browser CI starts a disposable API project and production preview,
checks readiness, preserves logs, and supports an opt-in multiplot run. The
multiplot wait now passes its 180-second timeout in Playwright's options slot.

Local numerical capture passed on Linux: 1.181 seconds and 19,386,368 bytes
process peak RSS. This single run includes process launch overhead and has no
accepted threshold. Success, nonzero exit, and launch-failure capture checks
passed. The existing benchmark cancellation text is preserved verbatim; its
`cancelled` flag means an error was returned, not a separately checked error code.
Browser integration verification is pending.
