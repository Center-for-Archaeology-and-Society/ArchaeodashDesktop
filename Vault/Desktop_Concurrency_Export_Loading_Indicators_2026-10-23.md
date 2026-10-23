# Desktop concurrency, group export, and loading indicators (2026-10-23)

Up: [[Index]]

## Why Explore was still slow
- Earlier caching ([[Explore_Performance_Caching_2026-10-23]]) was measured against a stubbed API. In the desktop app, every Tauri command was a plain (non-async) command, so Tauri 2 ran it on the main thread, one at a time, holding one `Mutex<DesktopState>` for the whole call. The Data Manager's background row preload and PCA/UMAP/LDA precompute therefore froze the window and queued every Explore request behind them.

## Changes
- `apps/desktop/src-tauri`: managed state is `SharedState = Mutex<Arc<DesktopState>>`. Commands are `async`, lock only to clone the `Arc`, and run their work with `spawn_blocking` (`run` helper). Project switching swaps the `Arc`. Source upload also runs off the main thread.
- `crates/desktop`: per-service `Mutex<Option<Service>>` → `RwLock` (read for calls, write on open), so calls to the same service run in parallel. Services are stateless (project root only) and already run concurrently behind the HTTP server; file locks coordinate writers.
- Explore: the missing-value profile is computed in the page from loaded rows (`explore/missing.ts`, same rule as the Rust port; no backend call). The "%" column now shows percent (it showed the fraction). Every element's default histogram and the profile plot (when rows × elements ≤ 250,000) warm in the background through the result cache.
- Export: `ExportMeasuredDataRequest.format` (`csv` default | `xlsx`); `ExportResult.content_encoding = "base64"` for binary. XLSX (rust_xlsxwriter): "All groups" sheet with the Group column plus one sheet per group (single group: one sheet); typed cells, so no formula guard is needed; valid unique sheet names; rejects > 1,048,575 rows. Desktop save dialog decodes base64 and filters by extension; the browser decodes too.
- Data Manager: **Export…** exports the highlighted groups, or every group included in analyses when nothing is highlighted; Excel (default) or CSV. Explore's table has Export to Excel / Export CSV.
- Loading indicators: `ui/Busy.tsx` (`Spinner`, `BusyNote`, `BusyLabel`) on all Loading/Computing lines and busy buttons (assign units, move/assign confirmations, duplicate/merge/delete, setup save, open project, precompute rows). `activity.ts` + `shell/ActivityIndicator.tsx`: every transport call is tracked and the top bar shows a spinner chip with what is running ("Assigning units to groups…") and a thin animated bar; dialogs and bookkeeping calls are excluded; shown after 250 ms.

## Verification
- Rust workspace tests and clippy pass on home2 (new: two-group XLSX read back with calamine, sheet-name rules). The Tauri shell crate is not in the Linux check (GTK); it compiles in the Windows build.
- Web: typecheck and 164 tests pass (missing profile, activity labels/tracker, export buttons, busy markup).
- Not checked by hand in the running desktop app.

Related: [[Windows_Cross_Build_2026-10-22]], [[Home2_Build_Wrappers_2026-10-23]]
