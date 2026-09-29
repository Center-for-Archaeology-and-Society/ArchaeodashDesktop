# Initial desktop test drive

Goal: a runnable desktop app with a basic import/Data Manager flow, disposable sample project, simple launcher, and native smoke-test evidence.

Parallel Luna tasks cover import UI and launcher/sample setup; root integrates and verifies native runtime behavior. Readiness is not assumed from compilation alone.

Related: [[Phase_6_Remaining_Controls_Jobs_2026-09-28]], [[Phase_6_Desktop_Project_Selection_2026-09-28]].

## Import and launch checkpoint

The Data Manager route now uploads supported source files, previews group partitions, chooses measured/ID roles, imports either by a source group column or into one named group, and validates discovered groups. Existing column-based contracts stay compatible. The sample generator creates five INAA groups without overwriting an existing directory; the launcher resolves a working pinned pnpm, builds frontend assets, and launches Tauri directly. Commits: `cd61a6c`, `4de001c`. Native window launch and folder selection have been exercised; complete import/analysis/export/reopen verification is underway on an isolated display.

## Native workflow checkpoint

On an isolated Linux X11 display, the actual Tauri app opened the INAA sample, validated all five groups, ran k-means on D1 (104 rows), imported a synthetic CSV into a new validated eight-row group, preserved the current project after cancelling the folder picker, and reported a cancelled UMAP analysis. Browser import verification passed eight cases; see [[Desktop_Import_Playwright_Smoke_2026-09-28]].

Native export review found the client still used browser-only Blob downloads. A Tauri save command now validates the suggested basename and 32 MiB payload cap, opens a native CSV save dialog, and publishes via same-directory temporary write/sync/rename on a blocking worker. Cancel writes nothing; focused failure and payload-limit tests pass. Final actual save/reopen verification follows.

## Ready for an initial local test

The full launcher reached the actual native window. Native CSV saving passed byte-value comparison for all eight imported rows; cancelling a second save preserved the file hash. Closing the window through its normal close protocol and relaunching rediscovered the five sample groups and imported group; the saved CSV was unchanged. The default clean sample is prepared at `/tmp/archaeodash-desktop-test-drive`. The guide and native screenshot are in [desktop-test-drive.md](../docs/operations/desktop-test-drive.md).

Verification: 223 Rust tests, 93 web tests, 38 client tests, workspace typechecks, production build, formatting, and affected-library Clippy pass. Browser import smoke passed eight cases. One direct accessibility-popup automation attempt terminated WebKit; ordinary pointer/keyboard selection and subsequent native save/reopen checks passed. The guide records this limitation; initial test readiness does not imply native stability certification or completion of the larger Phase 6 acceptance gates.
