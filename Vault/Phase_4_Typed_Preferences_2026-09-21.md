# Phase 4 Typed Preferences Surface — 2026-09-21

Related: [[Phase_4_Result_Exports_Golden_14_2026-09-21]], [[Phase_4_Desktop_Ordination_Tauri_Adapter_2026-09-20]]

## Summary

Second Phase 4 slice of the day, scoped by an explore agent and implemented
after the exports slice landed: the last named Phase 4 backend item —
**preferences** (IMPLEMENTATION.md Section 16; legacy `R/userPreferences.R`)
— through contracts, application, HTTP, and Tauri adapters. Commit `68d88f1`.

## What was built

- `crates/contracts`: `PreferenceKey` closed enum (typed allowlist per
  Section 10.1) with camelCase wire names matching the legacy fields —
  `theme` (`simple`/`light`/`dark`), `lastOpenedDataset` (non-empty, ≤255),
  `columnVisibility` (object of bools), `compactMode` (bool);
  `PreferenceEntry`/`GetPreferencesResponse`/`PutPreferenceRequest` with
  round-trip tests.
- `crates/application/src/preferences.rs`: `PreferenceService` persisting one
  JSON document at `.archaeodash/preferences.json` (project-scoped; the hosted
  per-user Section 6.5 control-plane table replaces this in Phase 7). Legacy
  parity: missing/corrupt store reads as empty and an upsert heals it
  (`read_user_preferences_safe` never-throw + last-resort rewrite); upsert
  replaces keys without duplicates; unknown stored keys are dropped on read.
  Writes go through `write_atomic` (now `pub` in `data-io`: unique tmp name,
  fsync before rename, cleanup on failure).
- `crates/api`: `GET/PUT /api/v1/preferences` + empty-store/round-trip/422
  test. `crates/desktop` + `apps/desktop/src-tauri`: `DesktopPreferences`,
  `preferences_get`/`preferences_set` commands registered in
  `generate_handler!`.

## Verification

- `cargo fmt --check`, clippy `-D warnings`, 150 workspace tests,
  `cargo check -p archaeodash-desktop-app` all green.
- Reviewer subagent initially REJECTED (corrupt store blocked writes),
  fix applied and re-review returned APPROVED. Deferred nit: concurrent
  HTTP PUTs are last-writer-wins per key document (read-modify-write),
  matching legacy; per-key locking can come with Phase 7 control plane.

## Status

Phase 4 backend items (PCA/LDA/UMAP, Explore views, descriptive edit,
exports, preferences) are now complete through both adapters. Remaining:
React client surface (packages/client is still a smoke skeleton) — the
Phase 4 exit needs numerical/visual parity on the client; then Phase 5.
