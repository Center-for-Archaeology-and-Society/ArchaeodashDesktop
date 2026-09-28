# Phase 5 Interactive Multiplot Plotly Mode — 2026-09-24

Phase 5 remainder item "interactive multiplot mode (Plotly + sampling label)"
landed, completing the follow-up flagged in [[Phase_5_Visualize_Assign_Slice_A_2026-09-23]].

## What landed

- **multiplot-model.ts**: added `samplingStatusText(plan, total)` — renders
  `Sampled X of Y points (stride N, deterministic)` only when the 100k
  ceiling dropped points (Section 3.2 parity) — and
  `uuidsFromSelectedEvent(eventData)`, the pure `plotly_selected` →
  uuid-array extractor (customdata[0], never rendered).
- **Multiplot.tsx**: new `Render mode` toggle — `Static (SVG)` (default,
  unchanged) vs `Interactive (Plotly)`. Interactive renders one Plotly
  `scattergl` figure per facet pair (per-panel figures keep DOM size bounded
  and preserve the progressive chunk-of-12 + continue control in both
  modes), colors by group with the same `colorFor` palette, carries
  `analytical_uuid` in `customdata`, shows group-only `hovertemplate`,
  lasso/box selection via `plotly_selected` and clear via
  `plotly_doubleclick`. New optional props `rowUuids`, `onSelect`,
  `initialMode` — existing call sites keep working.
- **Save plots**: both modes export SVG. Static keeps the XMLSerializer data
  URL path; interactive uses `Plotly.toImage({ format: 'svg' })` per figure.
- **VisualizePage.tsx**: minimal additive wiring only — passes `rowUuids`
  and `onSelect={(uuids) => setSelection(replaceSelection(uuids))}` so
  multiplot lasso feeds the same selection state as the single plot.

## Gotchas

- Plotly is dynamically imported inside the layout effect (same pattern as
  VisualizeScatter) so node:test `renderToString` never touches `window`;
  `initialMode="interactive"` lets SSR tests render the placeholder panels.
- SSR inserts `<!-- -->` between interpolated text nodes — strip comment
  nodes before asserting on button text like
  `Continue rendering 44 more panels` in tests.
- `tools/pnpm-build-test.sh` fails at its corepack shim
  (`~/.cache/node/corepack/pnpm/12.4.1/bin/pnpm.cjs` missing); run
  `pnpm -r build && pnpm -r test` directly with the pnpm on PATH.

## Tests

- `multiplot-model.test.ts`: sampling status text (null when unsampled,
  exact stride text at 100,001 rows), selected-event uuid extraction
  (empty/null events, non-array customdata skipped).
- `Multiplot.test.tsx` (new): toggle present with static default; no plotly
  placeholders in static SSR; interactive SSR-safe with aria labels and no
  uuid leak; sampling label in both modes; progressive chunk control.
- Gates: `pnpm -r build` and `pnpm -r test` (38 web tests) pass;
  `tsc --noEmit` clean. crates/ changes in the worktree belong to the
  parallel Phase 6 agent.

Back to [[Interaction_Log_2026-09-24]].
