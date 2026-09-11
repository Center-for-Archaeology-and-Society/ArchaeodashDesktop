# IMPLEMENTATION Section 9 - Client application implementation

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 9. Client application implementation

### 9.1 Technology choices

- React + TypeScript with Vite, managed by pnpm and a pinned Node LTS declared in `.nvmrc`/Volta/asdf.
- React Router for URL-addressable web navigation; desktop uses the same routes with hash/history configuration suitable for bundled assets.
- TanStack Query for remote/IPC resources and mutations.
- Zustand for small local workflow state; no monolithic global mirror of every dataframe.
- TanStack Table + TanStack Virtual for editable/virtualized tables, filters, column visibility, compact mode, and checked-row state keyed internally by `AnalyticalUuid`.
- Plotly.js for interactive plots, selections, WebGL scatter, and browser image export. Rust returns data/tree/model DTOs, not pre-rendered ggplot objects.
- Accessible component primitives and CSS variables owned by `packages/design-system`; no vendored Bootstrap 3/jquery/jquery-ui runtime.
- Zod validation generated/aligned from Rust/OpenAPI contracts at untrusted client boundaries.

### 9.2 Transport abstraction

Components call a `BackendPort`, never `fetch` or Tauri APIs directly:

```ts
interface BackendPort {
  projects: ProjectService;
  files: UserFileService;
  groups: GroupService;
  workspaces: WorkspaceService;
  transformations: TransformationService;
  analyses: AnalysisService;
  exports: ExportService;
  preferences: PreferenceService;
  jobs: JobService;
}
```

- `HttpBackendPort` uses relative `/api/v1`, CSRF header, problem-details errors, Arrow streams, and SSE job events.
- `TauriBackendPort` invokes typed Tauri commands and subscribes to job-progress events.
- Contract tests run the same behavioral suite against both adapters.

### 9.3 App shell and navigation

- Collapsible Data Manager remains at left on wide screens and becomes a drawer/below-content control on narrow screens.
- Main routes: Home, Explore, Visualize & Assign, Ordination, Cluster, Probabilities and Distances, Euclidean Distance, Info.
- Info subroutes: Help, Terms, Privacy. Render help in the app rather than an iframe; retain “open help separately” for web and a dedicated desktop window/browser action.
- Preserve Simple, Light, and Dark themes. Store anonymous/desktop choice locally; when authenticated, server preference wins after hydration. Avoid the current startup race that sends the local default before the saved preference is read.
- Keep loading stage/detail messages, nonblocking notifications, confirmation dialogs, compact result tables, responsive visualization filter placement, and plot expansion.
- Use semantic buttons/labels, keyboard access, focus management in dialogs, screen-reader status regions, and WCAG AA contrast. Test at keyboard-only, 200% zoom, and narrow viewport.

### 9.4 Feature route mapping

`Data Manager`

- Project area: create/open a directory; recursively show Parquet candidates with path, footer/profile state, checksum/change state, validation summary, editability, provenance, and actions.
- Candidate badges distinguish **Ready to add** (required metadata found; full validation pending), **Validated**, **Loaded**, **Not ready**, **Unsupported profile**, **Changed—revalidate**, and **Invalid**. Never imply that a metadata-only scan is full validation.
- Import preview wizard: choose a spreadsheet anywhere inside the project and sheet/range; review normalized names/types; select ANID, optional group, elemental and descriptive columns; configure import value policies/units; preview group partition/counts and filenames; then atomically create one group file per group. Without a group column, collect one new group name.
- Hosted project/file catalog provides equivalent logical paths, multi-select, explicit refresh, last-opened behavior, load progress, rename/move, collision handling, soft delete, quota status, and source download when present.
- Group Manager: validate/add/unload; create/rename/duplicate/archive; copy in groups from another research project; select active groups; inspect schema/provenance; switch a reference group into editable mode or create an editable local clone; move/copy selected analytical units; split/merge; and undo/recover supported revisions.
- Group/predictor selector with group counts, all/selected/unselected views, select/deselect/invert/reset.
- Ratio builder with preview and mode.
- Transformation draft, named save/overwrite, lazy load, delete, run flags, seed/advanced settings, status/cancel.
- Add/edit descriptive-column dialog with overwrite confirmation; elemental columns remain locked; reset predictors; clear workspace.
- Separate Save Project Settings, source reveal/download, Export Group, result export, and plot export actions so users can tell group persistence from calculated outputs.

`Explore`

- Virtual table; allow individual or batched descriptive edits with revision check and show save status. Hide `analytical_uuid`; measured elemental cells are read-only with an explanation.
- Missing-value count chart from original measured predictors, never a persisted imputed matrix.
- Crosstab counts on two fields or mean/median/SD of a numeric-convertible second field grouped by the first.
- Histogram field and bins 2–100, default 30.
- compositional profile lines across ordered predictors, colored by current groups where available.

`Visualize & Assign`

- Source selector; X/Y selectors with PCA variance labels; reject same axis.
- Plotly lasso/box selection keyed internally by `AnalyticalUuid`; double-click clears; selected-row table shows ANID, metadata, and predictors but not the UUID.
- Metadata field/value filter with `(Missing)` normalization and clear action.
- Viridis/default palette, optional data ellipse 0.50–0.99, symbol metadata field with conservative repeating ten-symbol map, optional labels with ANID/sample ID/displayed row-number fallback.
- Existing/new group target and assignment mutation.
- Multiplot X/Y disjoint selectors, point size, height 500–2000, static/interactive mode, progress/cancel, and export. Preserve the 100,000-point interactive sampling ceiling, but label when sampling occurs and make it deterministic.

`Ordination`

- PCA scores/cos², loading contribution, eigenvalues, contribution table.
- UMAP embedding view (resolves current orphan output).
- LDA availability status and vector view.

`Cluster`, `Probabilities and Distances`, and `Euclidean Distance`

- Preserve all controls/result/assignment behavior in Section 8.
- Checked rows remain selected through sort/filter/column-visibility changes because selection is a `Set<AnalyticalUuid>` outside table rows.
- Column visibility and compact settings persist as non-sensitive preferences.
