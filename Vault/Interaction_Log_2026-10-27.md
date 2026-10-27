# Interaction Log 2026-10-27

Up: [[Interaction_Log]]

- Launched the installed Windows desktop app (`%LOCALAPPDATA%\Programs\ArchaeoDash\ArchaeoDash.exe`, build from 2026-10-26); window opened normally.
- Reviewed parallelism: distance tables, k-means starts, PAM/HCA/DIANA, silhouettes, UMAP kNN and Euclidean membership use all cores (`pairwise::par_row_blocks`/`par_fill_rows`); membership Mahalanobis/Hotelling jackknife (`membership.rs`) is serial and recomputes each group covariance per row; UMAP SGD and imputation are serial for R parity; `umap.rs` toggles faer global parallelism (race across concurrent jobs). No code changed.
- Membership probabilities: each group's covariance/inverse is shared by all rows outside the group and rows run in parallel, bit-identical to before; UMAP eigensolver now passes `Par::Seq` directly instead of toggling faer's global setting. Analysis (62), application (117) and R parity tests pass; not committed, exe not rebuilt. See [[Membership_Shared_Covariance_Parallel_2026-10-27]].
- Committed the membership speed-up, UMAP eigensolver fix, and these Vault notes with `commit.sh` (no push); Windows exe not rebuilt yet.
- Assigning observations to an existing group no longer changes its inclusion (an unchecked target stays unchecked); new groups are still auto-included. Fixed in Visualize and analysis assignment via `includedAfterMove`/`createdGroups` in `multi-source-move.ts`; web checks pass (277 tests); not committed, exe not rebuilt. See [[Assign_Keeps_Existing_Group_Inclusion_2026-10-27]].
- Committed the assign-keeps-existing-group-inclusion fix and its Vault note with `commit.sh` (no push); Windows exe not rebuilt yet.
