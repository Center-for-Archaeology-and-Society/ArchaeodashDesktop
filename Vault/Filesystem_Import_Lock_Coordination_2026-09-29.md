# Filesystem Import Lock Coordination — 2026-09-29

`ImportService` now holds an `FsGroupFileStore` for its project. `commit` acquires a public RAII exclusive project lock before checking destination-name collisions and publishing validated group files, coordinating the existing write loop with store transactions and readers. `scan` uses the store's shared-lock candidate snapshot, like `GroupService::scan_candidates`.

The lock coordinates cooperating application/store code only. External filesystem writers that do not acquire `.archaeodash/project.lock` remain uncoordinated. A focused regression verifies import commit waits behind a store-held exclusive lock before publishing.

Related: [[Filesystem_Project_Scan_Lock_2026-09-29]], [[Filesystem_Group_Read_Locks_2026-09-29]], [[Phase_6_Service_Integration_2026-09-28]].
