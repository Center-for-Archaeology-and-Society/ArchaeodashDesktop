Up: [[Index]]

# Multiplot: responsive pickers and pop-out windows (2026-10-30)

Follows [[Phase_5_Interactive_Multiplot_Plotly_2026-09-24]].

## Problem

While the multiplot grid was still drawing, the X/Y pickers would not take a
single element or a range of elements. Every picker click re-rendered every
drawn panel (each static panel is one SVG circle per point), and the next
drawing step kept the main thread busy.

## Changes (`apps/web/src/visualize/`)

- **Pickers stop drawing at once.** Pointer or key input on the X/Y pickers
  stops the progressive draw and drops a step that is still rendering
  (`renderCount` goes back to what is committed). The grid follows the
  pickers 500 ms after the last change (`PICKER_SETTLE_MS`), then draws again.
  The pickers keep their own draft choice, so clicks show at once.
- **Panels are memoized** (`MultiplotPanel` is `memo` on plain, stable props),
  so picker clicks and other changes never redraw a finished panel.
- **Drawing steps are React transitions**, so React pauses a step's render to
  handle input. Static steps draw at most ~60,000 points
  (`panelsPerStep`: 12 panels, fewer for large data); interactive (WebGL)
  steps stay at 12. Removed Plotly panels are purged to free WebGL contexts.
- **Plot mode needs Elements axes.** On Visualize & Assign the Plot mode
  select is disabled for PCA, UMAP and LDA (multiplot draws element columns
  only), and switching to them returns to the single plot. The page's X/Y
  axis selects are hidden while the multiplot shows (it has its own pickers).
- **Open in new window** (`multiplot-window.ts`, `MultiplotWindow.tsx`): a
  layout choice, "All plots in one window" or "One window per plot" (at most
  24 windows, raised to 36 on 2026-11-04), and an open button.
  - The plotted columns and filtered rows are saved to IndexedDB
    (`archaeodash-multiplot`, Float64Array columns; entries older than a day
    are dropped on the next save) under a random key.
  - Windows load `/multiplot-window/<key>` (whole grid, with its own controls)
    or `/multiplot-window/<key>/<x>-<y>` (one plot sized to the window, with
    render mode, point size and save). `main.tsx` boots these without a
    backend, like the Help window, and follows theme changes.
  - Lasso selections in a window go back to the opening page over a
    BroadcastChannel; the page accepts only keys it opened.
  - Browser: windows open inside the click (pop-up blockers), then the data is
    saved and the window waits up to 15 s for it. If the browser blocks
    windows, the page says how many and asks to allow pop-ups.
  - Desktop: new Tauri command `open_multiplot_window` opens a webview window
    per plot. It accepts only `multiplot-*` labels and the
    `multiplot-window/<key>[/<x>-<y>]` route (unit-tested validators).

## Checks

- `scripts/home2/check.sh web`: 318 tests pass (new: step sizing, window
  controls, snapshot contents, route parsing, Plot mode enabled on Elements).
- `scripts/home2/run.sh clippy-desktop`: clean.
- Browser check on the home2 preview (`?demo`): Plot mode disabled on PCA;
  one X element gives 7 panels; touching a picker mid-draw shows "Stopped
  after 24 of 56" and drawing resumes after settling; both window views
  render from a saved snapshot. Real pop-ups were not opened (the agent
  browser blocks pop-ups from automated clicks), and the desktop window was
  not opened in the Windows app.
