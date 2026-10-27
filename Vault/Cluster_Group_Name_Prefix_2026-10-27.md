# Cluster groups: custom name prefix (2026-10-27)

Up: [[Index]]

## Request
When cluster results are assigned to groups, let the user set the text before each cluster number. The default stays `Cluster ` (`Cluster 1`, `Cluster 2`, ...); for example, `Cluster 5-` gives `Cluster 5-1`, `Cluster 5-2`, ...

## Change
- Contract: `ApplyClustersRequest.name_prefix` (optional; Rust `Option<String>` with `serde(default)`, TS `name_prefix?: string`). Absent means `DEFAULT_CLUSTER_NAME_PREFIX` (`"Cluster "`, in `crates/contracts`). Older callers keep working; the Tauri command and `POST /api/v1/groups/apply-clusters` pass the DTO through unchanged.
- Backend: `cluster_name_prefix` in `crates/application/src/groups.rs` checks the prefix before anything is read or written, then `apply_clusters` names groups `<prefix><k>`.
  - Rules follow `validate_folder_name`: not empty or only spaces, at most 100 characters, no leading dot, no control characters or `< > : " / \ | ? *`, and not a Windows-reserved file (`com` gives `COM1`). The reserved-name check is now the shared `windows_reserved` helper.
  - Leading spaces are dropped; a trailing space is kept.
  - Clash rules are unchanged: in Replace mode a name kept by a group that stays gets ` (2)`, ` (3)`, ...; file names come from `sanitize_group_name` (`Cluster 5-1` → `Cluster_5-1.parquet`).
- Web: `ClusterGroups.tsx` has a "Group name prefix" field, prefilled with `Cluster `. The description shows the resulting names. An invalid prefix shows the reason and disables Review; the field is locked while confirming. `cluster-groups.ts` mirrors the backend rules (`clusterNamePrefixProblem`) and sends `name_prefix`.

## Verification
- Rust (home2):
  - `cluster_name_prefix_defaults_and_refuses_unsafe_names` and `apply_clusters_uses_the_name_prefix` in application: custom prefix names and files, explicit default, a clash suffix, and an invalid prefix refused with nothing written.
  - `apply_clusters_name_prefix_is_optional` in contracts.
  - fmt, tests and clippy pass for contracts, application, api and desktop.
- Web: `cluster-groups.test.ts` (default, custom and invalid prefixes) and the new `ClusterGroups.test.tsx` (field, names, blocked Review); typecheck and 287 web tests pass.
- Committed without push; the Windows exe was not rebuilt.

Related: [[Cluster_Groups_Replace_Or_New_Folder_2026-10-26]], [[Interaction_Log_2026-10-27]]
