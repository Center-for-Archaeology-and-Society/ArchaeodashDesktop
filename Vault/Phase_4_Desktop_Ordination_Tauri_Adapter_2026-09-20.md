# Phase 4 Desktop Ordination Tauri Adapter — 2026-09-20

Related: [[Phase_4_UMAP_Naive_Port_Golden_07_2026-09-20]], [[Phase_4_Explore_Views_Golden_12_2026-09-19]], [[Phase_4_Ordination_PCA_LDA_Research_2026-09-17]]

## Summary

Closed the slice that both prior Phase 4 notes deferred: PCA, LDA, and UMAP
are now reachable through the Tauri desktop adapter, not HTTP only. The
desktop layer stays pure delegation to the shared `OrdinationService` core,
matching the HTTP routes in `crates/api` (Section 10.4 command names
`ordination_pca` / `ordination_lda` / `ordination_umap`).

## What was built

- `crates/desktop`: `DesktopOrdination` (`Mutex<Option<OrdinationService>>`,
  `open_project`, `with_service` mapping `DomainError` to IPC strings) with
  the three command bodies; doc comments cite Section 15.4 procedures 6/8/7
  and the Section 5 ephemerality invariant.
- `apps/desktop/src-tauri`: three `#[tauri::command]` wrappers, the
  `ordination: DesktopOrdination` state field, and `generate_handler!`
  registration after the explore commands.
- Test `ordination_commands_run_against_committed_groups`: no-project-open
  gate, 3-group CSV import + merge for LDA's legacy three-group minimum,
  PCA over the merged file with a byte-identical group-file assert
  (ordination never persists), UMAP on a separate 20-row fixture (legacy
  `n_neighbors = 15` needs > 15 rows), and a missing-file error path.

## Verification

- `cargo fmt --check`, `tools/cargo-lint.sh` (clippy -D warnings),
  `cargo test --workspace` (133 suites/tests pass),
  `cargo check -p archaeodash-desktop-app` all green.
- Reviewer agent approved: state wiring and `generate_handler!` complete;
  the `umap.rs` clippy fix (`is_none_or`) confirmed semantically identical
  for `usize`; nits noted (LDA <3-group rejection and LDA/UMAP byte-identity
  asserted at the core, not re-asserted at the desktop layer).
- Environment note: rustfmt/clippy and the Tauri check's system libraries
  (pkg-config, libdbus, gdk-pixbuf, gtk3, webkit2gtk dev) had to be installed
  before the gates ran.

## Status

Phase 4 remaining: Explore UI preferences/exports (per
[[Phase_4_Explore_Views_Golden_12_2026-09-19]]); the client (React) surface
itself is still pending across all phases. Ordination backend + adapters are
now complete (goldens 6, 7, 8; HTTP + Tauri).
