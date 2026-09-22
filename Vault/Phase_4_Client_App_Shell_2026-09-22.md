# Phase 4 Client App Shell — 2026-09-22

Related: [[Phase_4_Client_Transport_Port_2026-09-22]], [[Phase_4_Typed_Preferences_2026-09-21]]

## Summary

Second Phase 4 client slice today: the Section 9.3 React app shell in
`apps/web`, replacing the smoke-skeleton entry. Commit `f6286b0`.

## What was built

- **Shell** (`apps/web/src/shell/AppShell.tsx`): top navbar in the legacy
  tab order — Home, Explore, Visualize & Assign, Ordination, Cluster,
  Probabilities and Distances, Euclidean Distance, Info (dropdown: Help /
  Terms & Conditions / Privacy Policy) — plus the collapsible Data Manager
  sidebar (`aria-expanded`/`aria-controls`, drawer-below-content under
  50rem) and the theme `<select>`.
- **Themes** (`packages/design-system`): tokens now carry the exact legacy
  `styles.css` palette for light/simple/dark (bg, text, accent gradient,
  sidebar gradient, panel, muted) with `themeCssVariables()` emitting the
  legacy custom-property names; `normalizeTheme` keeps the legacy JS
  `simple` fallback. `theme.ts` paints the stored choice before first
  render and hydrates the persisted preference after mount (Section 9.3
  startup-race correction).
- **Routes** (`shell/routes.tsx`): Info pages render the legacy
  `www/help.md`/`terms.md`/`privacy.md` in-app via `marked` (no iframe,
  per Section 9.3; markdown copied to `src/content/`). Explore and
  Ordination render structured placeholders to be filled by the next
  slices; Phase 5/6 routes are placeholders until their phases.
- **Transport selection** (`src/transport.ts`): Tauri IPC via
  `window.__TAURI_INTERNALS__` dynamic import of `@tauri-apps/api/core`,
  HTTP (`/api` relative base) elsewhere. Vite dev proxy now also covers
  `/healthz` (the API serves it at the root, so the old `/api` base URL
  would have missed it).
- **Tooling**: `scripts/ts-hooks.mjs` gained an esbuild branch for `.tsx`
  (amaro strips types only, no JSX) and vite-style `?raw` markdown
  imports; `apps/web` test glob now covers `*.test.tsx`.

## Verification

- `pnpm -r typecheck`, `pnpm -r build` (378 kB bundle), `pnpm -r test`
  (33 tests: 5 new shell tests via `react-dom/server` `renderToString` —
  nav order/labels, theme palette + normalization, aria-wired collapse,
  in-app markdown, route placeholders) all green; Rust gates green
  (fmt, clippy per CI invocation, 150 tests).

## Notes

- No plot/table deps yet (TanStack Table/Query, Plotly) — they arrive with
  the Explore/Ordination data slices, keeping the shell commit reviewable.
- `renderToString` unit tests cover structure only; interactive behavior
  (collapse, theme switch) is typed and aria-wired for the later
  Playwright layers (Section 15.2).
