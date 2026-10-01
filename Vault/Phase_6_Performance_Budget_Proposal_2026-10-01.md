# Phase 6 Performance Budget Proposal — 2026-10-01

This note proposes portable Phase 6 performance budgets from the captured
cross-platform evidence, for owner ratification. Nothing here is ratified
until accepted; the runners still report `budgets.status: not_configured`
unless explicit limits are supplied.

## Captured evidence (2026-10-01, revision `294d1d5`)

Hosted `Phase 6 numerical performance evidence` run, release
`phase6_bench` example, process-wide timing:

| Platform | Elapsed | Peak RSS |
|---|---:|---:|
| Ubuntu x86_64 (azure) | 0.863 s | 22,474,752 bytes (~21.4 MiB) |
| macOS arm64 | 2.088 s | 32,931,840 bytes (~31.4 MiB) |
| Windows AMD64 | 1.808 s | explicitly unavailable (working-set sampling not implemented) |

Hosted `Phase 6 browser E2E` run with the multiplot benchmark enabled:
99,960 points across 12 panels (8,330 per panel), 7,341 ms including the
static multiplot mount, Chromium 151, ubuntu-latest. Local runs measured
3,016–3,608 ms on Linux x64.

## Proposed budgets

With roughly 4x headroom over the slowest observed hosted run:

- Numerical `phase6_bench`: elapsed ≤ 10 seconds on every platform; peak
  RSS ≤ 128 MiB on platforms where the runner can measure it (Linux/macOS).
  Windows dispatches set the elapsed limit only; a requested but unmeasurable
  memory limit correctly fails the check, so it is omitted there.
- Interactive multiplot render: elapsed ≤ 15 seconds for the full 99,960-point
  12-panel scenario including the static mount.

Rationale: these budgets bound regressions, not hardware minimums. The
observed spread between the fastest (0.863 s) and slowest (2.088 s) hosted
platforms is about 2.4x; 10 s leaves margin for slower shared runners while
still failing a gross regression such as an accidental quadratic path (which
would exceed the limit by an order of magnitude at the 1,000-row service cap).

## Enforcement

`scripts/benchmark-phase6.py --max-elapsed-seconds --max-peak-rss-bytes` and
`scripts/e2e/multiplot-100k.mjs --max-elapsed-ms` already fail nonzero and
retain structured results when limits are exceeded. The workflow-dispatch
inputs expose the same options. Ratification means recording the accepted
numbers here and dispatching hosted runs with them set.

## Native launch smoke (2026-10-01)

A new `Desktop launch smoke` CI job builds the Tauri debug shell with the
embedded production web assets and launches the real binary on windows-latest
and macos-latest, requiring the process to stay alive through startup
(25 seconds) with stderr/stdout captured on failure. Both platforms pass.
This is launch evidence, not interactive acceptance: the pointer-lasso
assignment walkthrough remains verified on Linux only.

Related: [[Phase_6_Hosted_Cross_Platform_Evidence_2026-09-30]],
[[Phase_6_Repeatable_Acceptance_Evidence_2026-09-30]],
[[Phase_6_Final_Acceptance_Work_2026-09-30]].
