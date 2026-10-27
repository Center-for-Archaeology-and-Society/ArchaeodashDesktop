# Cluster results become groups: replace or new folder (2026-10-26)

Up: [[Index]]

## Request
Assigning clusters should offer two choices, with no group names to type (users rename later):
- **Replace**: only the selected (analyzed) groups are replaced. With groups A and B in a folder and only B selected, B becomes Cluster 1, Cluster 2, ...
- **New folder**: the cluster groups go into a new folder.

## Bug fixed
Cluster assignment failed with "The analysis result has no safe source path or revision". `safePath` (`apps/web/src/clustering/new-group.ts`) rejected `\`, but Windows group paths look like `group2\A.parquet`. It now accepts both separators and still refuses `..`, absolute paths, drive letters, and empty parts.

## Backend: `GroupService::apply_clusters` (`crates/application/src/groups.rs`)
- New contract `ApplyClustersRequest { sources: [GroupRevisionRef], clusters: [[uuid]], mode: replace | new_folder }`.
  - Journaled as `TransactionAction::ApplyClusters`.
  - Exposed as the desktop command `apply_clusters` and `POST /api/v1/groups/apply-clusters`.
  - Client method: `groups.applyClusters`.
- One transaction:
  - Group names are `Cluster 1..k`; empty clusters are skipped.
  - Rows keep UUIDs, values and descriptive cells, in source order.
  - The sources must share a folder and a schema, and have current revisions.
  - The clusters must cover every source row exactly once; otherwise "run the analysis again".
- **Replace**:
  - Outputs go in the sources' folder, and the sources are deleted.
  - A name used by a group that stays (not selected) gets ` (2)`, ` (3)`, ...
  - A cluster file that reuses a selected group's file name overwrites it in place (revision bumped).
- **New folder**: `<folder> clusters` beside the source folder (then ` 2`, ...; `Clusters` at the project root), created with the folder marker and removed again if the transaction is refused. The sources are untouched.
- The per-folder unique-sample check applies as for every write.

## Web
- `cluster-groups.ts` builds the request (partition labels, or the dendrogram cut via `clusterOfRow`).
- `ClusterGroups.tsx` handles review and confirm.
- `ResultAssignment` assignment modes for cluster fits:
  - "Move checked samples to one group" (unchanged)
  - "Replace the analyzed groups with the clusters"
  - "Create a new folder of cluster groups"

  The old "Use analysis groups" label mapping stays for membership and Euclidean results, and for cluster fits when the transport lacks `applyClusters`.
- After a new-folder run, the notice offers "Analyze the new folder" (switches the active folder).
- The clustering result stays valid after either mode (same samples).

## Verification
- Rust test `apply_clusters_replaces_selected_groups_or_writes_a_new_folder` covers:
  - a new folder, and a second run (` 2`)
  - replacing one of two groups
  - a name clash suffix
  - re-clustering cluster groups (in-place overwrite)
  - refusals: stale revision, missing sample, sample in two clusters
- Web tests: `cluster-groups.test.ts` and the Windows-path cases in `new-group.test.ts`.
- Workspace tests, clippy, fmt and web checks pass on home2; the Tauri app crate compiles for Windows (`cargo xwin check`).

Related: [[Clustering_Dense_Methods_100k_2026-10-26]], [[Interaction_Log_2026-10-26]]
