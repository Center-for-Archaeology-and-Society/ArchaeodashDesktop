# Analysis reads wait for group saves (Windows "Access is denied") (2026-10-28)

Up: [[Index]]

## Problem
Unchecking a group while an assignment was still saving failed the save: "Assignment failed: io error: publish failed: Access is denied. (os error 5)".

The desktop log (`%LOCALAPPDATA%\org.archaeodash.desktop\logs\archaeodash.2026-10-28.log`, 14:55:03–04) shows `transfer_units` holding the project lock for 10 s. The uncheck started Explore reruns (`explore_histogram`, `explore_compositional_profile`) during that time. They failed with "internal error", and 0.2 s later the save's rename failed.

## Cause
Store transactions publish under the exclusive `.archaeodash/project.lock`, and store reads (`read_group`, scans, snapshots) take it shared. But analysis inputs read group files directly and took no lock:
- `CombinedInput::read_local`, used by Explore, Ordination (PCA/UMAP/LDA), exports and transforms;
- `ClusteringService::read_group`.

So a rerun could open a group file the save was replacing. On Windows the rename over an open file fails with "Access is denied", and the reader can also see a half-published set of files.

## Fix
- `archaeodash-file-store-fs`: new `lock_project_shared(root)`, the same shared lock store reads use.
- `combined::read_locked` takes it around `CombinedInput::read_local` and around clustering's `read_inputs`. Reads now wait until a save finishes, then see the new files. The lock is released before the analysis runs. No path reads analysis input while holding the exclusive lock, so this cannot deadlock.
- Publishing (`execute` step 5) goes through `replace_file`. On Windows it retries a refused rename for under a second in total, for outside programs such as a virus scanner or indexer. The error now names the file: "publish failed: could not replace <path> (is it open in another program?): …".

## Verification
- `combined::tests::read_local_waits_while_a_change_is_being_saved`: a read started while the exclusive lock is held does not finish within 300 ms. It finishes after release and returns both files.
- home2 `check.sh rust archaeodash-file-store-fs archaeodash-application`: fmt, tests (application 121, file-store 16) and clippy pass.
- Not reproduced by hand in the Windows desktop app (exe not rebuilt).

Related: [[Filesystem_Cross_Process_Read_Lock_2026-09-30]], [[Filesystem_Group_Read_Locks_2026-09-29]], [[Rename_Single_Group_2026-10-27]], [[Interaction_Log_2026-10-28]]
