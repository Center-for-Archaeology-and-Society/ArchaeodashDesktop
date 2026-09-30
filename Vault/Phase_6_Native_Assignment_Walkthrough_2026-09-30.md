# Phase 6 Native Assignment Walkthrough — 2026-09-30

The prepared walkthrough has now passed in the actual Linux GTK/WebKit Tauri
window on an isolated X11 display. A real pointer lasso selected one INAA unit,
AID669, and the UI committed a D1-to-D2 move. The source changed 104→103 and the
destination 50→51; a read-only Parquet comparison preserved the hidden identity,
all 33 measured values, every descriptive field, and all other rows. After
close/relaunch and native folder reopening, Explore showed 103 source rows with
the unit absent and 51 destination rows with it present once.

The run exposed collapsed WebKit plot sizing that covered assignment controls.
An explicit scatter height fixes layout; awaited Plotly rendering and stable
callback references fix related lifecycle handling. A real-pointer Chromium
regression is included in CI. Linux native acceptance is now evidenced;
Windows/macOS and portable performance acceptance remain open.

See [desktop test-drive guide](../docs/operations/desktop-test-drive.md) for the
replay steps and native screenshot. Related: [[Phase_6_Final_Acceptance_Work_2026-09-30]], [[Phase_6_Remaining_Controls_Jobs_2026-09-28]].
