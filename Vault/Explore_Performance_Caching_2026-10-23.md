# Explore performance: caching and preloading (2026-10-23)

Up: [[Index]]

## Diagnosis (30,000 rows in 10 groups)
- Backend (release, home2): every request re-read and decoded the group files, and `read_group_file` decoded each file twice (validation, then rows). Load all rows 350 ms (+14 MB JSON), validate all 260 ms, missing profile 370 ms, histogram 320 ms — almost all I/O.
- Frontend: every page visit rescanned and refetched all included rows; each Explore tab asked the backend again; the Dataset table rendered all 30,000 rows with an input per descriptive cell and re-rendered the whole table per keystroke.

## Changes
- `crates/data-io/src/group_cache.rs`: in-process cache behind `read_group_file` / `validate_group_file`, keyed by canonical path + size + mtime + SHA-256 of the Parquet footer (holds the profile/revision and chunk metadata, so any save changes the key). LRU, 128 files / 1,000,000 rows; `ARCHAEODASH_GROUP_CACHE=off` disables. Benefits every caller (rows, validation, Explore, ordination, transforms, clustering).
- `apps/web/src/datasets/DataCache.tsx` (provider per open project in `main.tsx`): group rows keyed by path + revision (shared across pages; Reload bypasses), computed results keyed by request + file revisions (`useCachedResult`), and a background preload of all ready groups (included first, up to 500,000 rows) started by the Data Manager.
- Explore: all four views read through the result cache; the missing-value profile is computed as soon as data loads; keys use the autosave's current revisions (`CellAutosave.revisionsKey`) so results recompute after saved cell edits.
- Dataset table: above 200 rows it becomes a scrolling panel (70vh, sticky header) that draws only the rows in view plus 15 above/below, with measured row height, spacer rows, and `aria-rowcount`/`aria-rowindex`.

## Measurements
- Backend after the cache: validate all 0.2 ms, rows 101 ms, missing profile 61 ms, histogram 46 ms.
- Frontend, same stubbed 30k-row project, zero-latency API (UI time only), old vs new build: open Explore 2,434 → 114 ms; keystroke 770 → 1 ms; back to Dataset 2,039 → 41 ms; Missing values tab 432/575 → 2/2 ms; revisit Explore 2,629 ms with 10 refetches → 52 ms with none. Only 48 rows are drawn; scroll position maps exactly (29 px × 30,000).

## Verification
- Rust: workspace tests pass (incl. back-to-back same-length saves seen through the cache, `patch_descriptive_values_edits_by_uuid_and_bumps_revision`); clippy clean.
- Web: 156/156 tests (cache revision/sharing/failure/preload budget, windowed table).
- Not yet run in the desktop app (no rebuild).
