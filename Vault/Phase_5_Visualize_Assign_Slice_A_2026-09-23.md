# Phase 5 Visualize Assign Slice A — 2026-09-23

Phase 5 ("Interactive plot, ellipses/symbols/labels/filter/lasso/selected
table, assignment transaction, multiplots and plot saves") started.
Commit `cbacf56`.

## What landed

- **visualize-model.ts** (pure, unit-tested): uuid-keyed selection
  (replace/toggle/clear), `(Missing)` filter normalization, ten-symbol map,
  viridis anchors, label fallback (ANID → sample ID → row number), and the
  data ellipse via analytic 2×2 eigen-decomposition scaled by
  `r² = -2·ln(1-level)` (0.50 → 1.3863, 0.95 → 5.9915, 0.99 → 9.2103).
- **VisualizeScatter.tsx**: `plotly.js-dist-min` (v4.1.1, with types) is
  dynamically imported inside the layout effect so `renderToString` tests
  never touch `window`. `scattergl` traces per group; `customdata[0]` carries
  the analytical_uuid for lasso selection while `hovertemplate` shows only
  display label + group; dashed ellipse trace per group;
  `plotly_selected` → uuid array, `plotly_doubleclick` → clear.
- **VisualizePage.tsx**: dataset select, X/Y selectors (same-axis rejected
  with the legacy message), PCA-coordinates toggle using PCA variance labels,
  metadata field/value filter (row-index-based so it filters PCA scores too),
  ellipse level select (off/50/90/95/99), symbols-by-metadata, label mode,
  SelectedRowsTable (ANID, metadata, predictors — never the uuid), and the
  assignment transaction: existing target path or new group
  `groups/<sanitized>.parquet` via `transferUnits` (move) with
  `expected_source_revision`, rows reloaded after commit.
- **Routing**: `main.tsx` wires `VisualizeDeps { groups, ordination, exports }`;
  the `VisualizePage` placeholder left `routes.tsx`; shell test updated.
- **Bug found by tsc after the fix**: a stale Phase-1 scaffold
  `apps/web/src/main.ts` shadowed `main.tsx` in tsc's file list, so four
  real type errors in main.tsx (imports/deps wiring) were invisible.
  Deleted it; typecheck now covers the real entry point.

## Gotchas

- React SSR inserts `<!-- -->` between interpolated text and siblings —
  assert on stable fragments (`'selected'`), not `'{n} selected'`.
- `fe` line edits keep corrupting multi-line JSX; full-file `write` for
  new files and unique-anchor `edit` for patches.
- `transfer_units` new-destination semantics (crates/application/src/groups.rs:187):
  destination path must not exist; `destination_group_name` required;
  sanitized group id comes from `sanitize_group_name`
  (crates/data-io/src/group_profile.rs:121, keeps `[A-Za-z0-9._-]`).

## Next

- Phase 5 remainder: multiplots (X/Y disjoint selectors, point size, height
  500–2000, static/interactive, 100k-point deterministic sampling with
  sampling label), plot saves, e2e for uuid-hidden selection/assignment.
- Then Phase 5 exit gate: procedures 6–8 & 12 re-run + Section 15.2
  web/desktop e2e for assignment atomicity.

Back to [[Interaction_Log_2026-09-23]].
