# Filesystem Group Read Locks — 2026-09-29

`FsGroupFileStore::validate_group` and `read_group` acquire a shared advisory
lock on `.archaeodash/project.lock`. They therefore wait for in-process or
cross-process store transactions using the exclusive project lock, and
multiple readers can proceed together. `execute` uses an unlocked validation
helper while holding its exclusive lock to avoid recursive acquisition.

The lock coordinates cooperating `FsGroupFileStore` processes only. External
tools that modify project files without acquiring this lock remain outside the
coordination boundary; multi-file snapshots are also not provided.

The focused regression checks that a second store's reader waits during an
exclusive lock, succeeds after release, and can read concurrently with a
separate shared lock holder. Package Clippy with warnings denied passes.
