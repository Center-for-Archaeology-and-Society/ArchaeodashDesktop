# Filesystem Group Read Snapshots — 2026-09-29

`FsGroupFileStore::read_groups_snapshot` acquires one shared project lock and reads an ordered list of relative group paths under that lock. Existing groups are returned as data and missing paths as `None`, giving planners one consistent view for mixed existing and create-only destinations.

`GroupService::batch_transfer_units`, `transfer_units`, and `merge_groups` use the snapshot API. Their transaction inputs still carry the revisions/checksums observed during planning, and new destinations remain protected by the executor's create-only absence check while holding the exclusive project lock. The focused filesystem and application test suites pass; library-target Clippy with warnings denied passes. All-target Clippy currently reports existing `unwrap`/`expect` test lints in unrelated application modules.

Related: [[Filesystem_Group_Read_Locks_2026-09-29]], [[Phase_6_Service_Integration_2026-09-28]].
