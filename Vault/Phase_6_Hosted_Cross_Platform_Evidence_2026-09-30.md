# Phase 6 Hosted Cross-Platform Evidence — 2026-09-30

After pushing the long-local branch to origin/master, the hosted GitHub
Actions workflows produced the cross-platform Phase 6 evidence that was
previously pending.

## Workflow runs

- CI on `c8978ed`: all eight jobs green — numerical analysis/parity on
  Ubuntu, macOS, and Windows; the cross-process filesystem-lock regression
  on all three platforms; Rust fmt/clippy/full-workspace tests; TypeScript
  build/tests.
- Phase 6 browser E2E on both pushes: success (real API-backed Chromium
  cases, including the real-pointer assignment regression).
- Phase 6 numerical performance evidence (manual dispatch on `294d1d5`),
  all three runners green:
  - Ubuntu x86_64: 0.863 s benchmark elapsed, peak RSS 22,474,752 bytes.
  - macOS arm64: 2.088 s, peak RSS 32,931,840 bytes.
  - Windows AMD64: 1.808 s; peak RSS explicitly unavailable (working-set
    collection not implemented there, no threshold evaluated).

These are analysis-example captures, not full native-app budgets; no
portable threshold is ratified yet.

## Two hosted-CI fixes landed

- Golden/parity fixtures are now marked `-text` in `.gitattributes`. Windows
  checkouts converted `fixtures/golden/14_measured_data_export.csv` to CRLF,
  breaking golden #14's literal byte-equality export round trip; Linux/macOS
  passed because they never rewrite line endings.
- The Rust CI job now builds the web assets (`pnpm -r build`) before
  Clippy/tests, because `tauri::generate_context!` requires
  `web/dist` to exist, and frees runner disk space before the full
  workspace build (the first attempt died with "No space left on device"
  inside `cargo test --workspace`).

## Remaining Phase 6 gates

Native Tauri acceptance on macOS and Windows desktop builds and an agreed
portable performance budget remain open. Hosted numerical, browser, lock,
and lint/test evidence is no longer a blocker.

## Cross-platform desktop workspace tests (2026-10-01)

A new `Desktop workspace tests` CI matrix runs the full workspace — including
the Tauri desktop shell compile against `web/dist` — on macOS and Windows.
Two real portability defects surfaced and were fixed:

- `import.rs` compared `FsGroupFileStore` scan paths against a non-canonical
  tempdir path; macOS resolves `/var/folders` through `/private/var`, so the
  store's canonicalized root no longer matched. The test now canonicalizes the
  expected root (`4a85d02`).
- `tauri-build` requires `icons/icon.ico` to generate the Windows resource
  file; a multi-size ICO (16–256 px) derived from the existing placeholder
  color was added (`4a85d02`).

After the fixes, CI on `4a85d02` is fully green: all three platforms pass
numerical parity, lock regressions, and the desktop workspace tests, plus
Rust fmt/clippy/tests on Linux and the TypeScript build/tests.

This is compile-and-test evidence only; interactive native assignment
walkthroughs on macOS/Windows and a ratified portable performance budget
remain open Phase 6 gates.

Related: [[Phase_6_Repeatable_Acceptance_Evidence_2026-09-30]],
[[Phase_6_Final_Acceptance_Work_2026-09-30]],
[[Phase_6_Remaining_Controls_Jobs_2026-09-28]].
