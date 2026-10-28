# Impute from all groups and the prepared-values cache (2026-10-28)

Up: [[Index]]

## Request
Add an imputation option to use all groups, on by default (also for saved setups; the app has only been used for testing). With it, cache the imputed result so including or leaving out groups doesn't recompute it. Log values don't depend on the selection either, so they should be cached too. Use the home2 wrapper scripts to avoid permission prompts.

## Behaviour
- `TransformationDefinition.imputation_scope`: `all_groups` (serde default, so setups saved without it impute from all groups) or `included_groups` (the previous behaviour).
- Setup form: "Impute from all groups" checkbox under the seed (shown when an imputation method is chosen), with a hint that explains each state. The setup summary reads e.g. "PMM imputation (all groups)".
- The pool is every ready group file in the folder(s) of the analyzed groups that has all the selected elements. Other folders are regroupings of the same samples, so they are excluded.

## Backend (`crates/application/src/prepared.rs`)
- `pooled_imputation(pool, definition)` imputes the definition's elements once over the pool (canonical UUID order) and returns imputed values by sample UUID.
- Disk cache `.archaeodash/cache/prepared/<content>.json`, keyed by the settings plus every pool row's UUID and measured values. Moving samples between groups reuses it; edits or imports change it. Keeps 24 tables.
- In-memory index keyed by the folder listing (file names, sizes, modification times; nothing opened), so an unchanged pool is not re-read. One build at a time per process.
- `TransformService::apply_definition_in(..., pool)` substitutes the pooled values by UUID. Ratios and the transform run afterwards, so log and ratios are per sample and z-score still standardizes over the analyzed rows. Used by ordination, clustering, explore, exports and `transformations/apply`.
- Ordination cache keys (PCA/UMAP via `pool` in the key, LDA via the request hash) include the pool's content key when imputing from all groups. A change anywhere in the folder recomputes them, while moves still reuse them. Keys without folder-wide imputation are unchanged.
- `AppliedTransformation.analytical_uuids` (serde default) lets the client match rows to samples.

## Web
- `selectionIndependent(setup)`: a per-sample transform (not z-score) with no imputation or with all-groups imputation.
- Visualize element axes: when selection-independent, the transformed values are fetched once for every ready group in the folder, keyed by those files' revisions, and picked by sample. Including or leaving out groups sends no request, so the log values are effectively cached. Otherwise they follow the rows as before.
- Logs are not stored on disk separately: they are per-sample arithmetic that is cheaper than reading a cache. The saving is not re-reading and not re-imputing.

## Process
- New `scripts/home2/push.sh`: fetch, push master only as a fast-forward, never force. Not yet in `.claude/settings.local.json` (needs the user's OK).
- All checks ran through `bash scripts/home2/check.sh …` with no `cd …&&` prefix, so they match the allowlist.

## Verification
- `transforms::tests::imputation_from_all_groups_ignores_the_selection`: Baca alone and Baca+Hooper give Baca identical imputed values, holes filled, one pool table stored. Included-groups mode equals imputing Baca's own rows and stores nothing. Removing Hooper changes the pool key.
- `prepared::tests::pool_folders_come_from_request_paths`; web tests for the checkbox default (with and without the saved field), the summary, and `selectionIndependent`.
- home2: Rust (contracts, application, api, desktop) fmt/tests/clippy, web typecheck + 305 tests, and parity all pass.
- Not tried in the desktop app with a real project yet.

Related: [[Membership_Sorting_Names_Assign_Hold_Collapsible_2026-10-28]], [[Ordination_Precompute_Cache_2026-10-23]], [[No_Recompute_On_Group_Moves_2026-10-26]], [[Transformation_Pipeline]]
