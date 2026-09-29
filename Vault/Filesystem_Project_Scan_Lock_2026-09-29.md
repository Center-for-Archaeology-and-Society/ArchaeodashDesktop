# Filesystem Project Scan Lock — 2026-09-29

`FsGroupFileStore::scan_candidates_snapshot` holds one shared project lock across recursive candidate discovery and profile metadata reads. `GroupService::scan_candidates` uses this entry point, so cooperating store transactions wait until a scan completes and scans wait for active transactions.

This advisory lock only coordinates users of the filesystem store. External programs that modify project files without taking `.archaeodash/project.lock` remain outside the coordination boundary. A focused regression confirms a scan waits behind an exclusive project lock.

Related: [[Filesystem_Group_Read_Locks_2026-09-29]], [[Filesystem_Group_Read_Snapshots_2026-09-29]], [[Phase_6_Service_Integration_2026-09-28]].
