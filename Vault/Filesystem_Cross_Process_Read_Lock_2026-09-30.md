# Filesystem Cross-Process Read Lock — 2026-09-30

Added a regression that starts a child test process and verifies `read_group`
stays blocked while the parent holds the exclusive `.archaeodash/project.lock`,
then completes after release. Startup and completion have timeouts, and the
child is killed and reaped on failure. The test passed on the current Linux
host; formatting and file-store Clippy with warnings denied pass. A focused CI
matrix now runs this exact regression on Ubuntu, macOS, and Windows without
installing Tauri libraries; hosted results will provide the cross-platform
verification.

Related: [[Filesystem_Group_Read_Locks_2026-09-29]], [[Filesystem_Project_Scan_Lock_2026-09-29]].
