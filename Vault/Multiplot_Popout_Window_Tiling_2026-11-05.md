Up: [[Index]]

# Multiplot pop-out windows tile across the screen (2026-11-05)

Follows [[Multiplot_Responsive_Pickers_And_Popout_Windows_2026-10-30]]. "One window per plot" used to cascade the windows 30 px apart; now they tile.

## Behavior
- **On open** (one window per plot, 2+ windows): each window gets its own slot on the screen the app is on.
  - Full X × Y matrix: windows keep the multiplot's layout (row = Y column, column = X column; the X = Y diagonal stays empty).
  - Wrapped grid (one column on X or Y): windows fill the grid that best fits the screen (`fitColumns`: tile shape closest to 1.15:1, fewest empty slots).
  - Tiles never get smaller than 360 × 300; past that the windows keep that size and overlap, spread evenly to the screen edges.
- **Tile windows** button (appears after opening one window per plot): re-tiles every still-open plot window from this page. If all windows of a single open click are still open they keep their slots; otherwise the open ones fill a fitted grid. Shows a message if none are open.
- "All plots in one window" is unchanged (one cascaded window).

## Code
- `apps/web/src/visualize/multiplot-window.ts`: `screenArea`, `fitColumns`, `tileRects` (pure, tested in `multiplot-window.test.ts`), `openPopouts` returns a `PopoutGroup`, `tileWindows`.
- Browser: `window.open` gets the slot (page size minus an estimated 8 × 72 frame), then `moveTo`/`resizeTo` set the exact outer box. Chrome/Edge honor this for popups the page opened; Firefox can disable it (`dom.disable_window_move_resize`); Safari is unreliable. Single screen only (no Window Management API prompt).
- Desktop (`apps/desktop/src-tauri/src/lib.rs`): `open_multiplot_window` takes optional `x`, `y` (then width/height are the outer box; the frame is measured and the inner size corrected). New commands `multiplot_windows_open(labels)` and `place_multiplot_window(label, x, y, w, h)` (never creates a window). Coordinates are clamped to ±20000, sizes to 320–4000.
- `Multiplot.tsx`: targets carry `cell`; "Tile windows" button.

## Checks
- Web typecheck; web tests 337/337 (local, with esbuild copied off the noexec share via `ESBUILD_BINARY_PATH`).
- Desktop crate: `cargo clippy -D warnings`, fmt, multiplot tests pass (built with an empty `apps/web/dist` placeholder, removed after).
- Not checked by hand in a real browser or the desktop app.
