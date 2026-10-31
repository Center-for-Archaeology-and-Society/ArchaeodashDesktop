Up: [[Index]]

# Hosted project Trash (2026-10-30)

Deleting hosted projects, with a 30-day Trash. Builds on [[Hosted_Project_Workspaces_2026-10-28]]. Desktop projects are folders and are not affected.

## Behaviour
- **Delete** on the Projects page asks to confirm, then moves the project to the Trash (`projects.deleted_at` is set). A trashed project is 404 on every project and workspace route, like before.
- The project's files stay on disk while it is in the Trash. Before this change a delete removed the workspace folder at once, so nothing could be restored.
- **Trash** section on the Projects page: each deleted project with "Deleted permanently in N days", **Restore**, and **Delete now** (confirmed, cannot be undone).
- Deleting the open project closes it and goes back to the project list. The remembered last project is forgotten.
- Project names are not unique, so a restore never conflicts.

## API (owner only, 404 otherwise; CSRF on writes)
- `DELETE /api/v1/projects/{id}`: move to Trash (204).
- `DELETE /api/v1/projects/{id}?permanent=1`: delete for good, live or trashed (204).
- `GET /api/v1/projects/trash`: `{ projects: [{ project_id, name, created_at, updated_at, deleted_at, purge_at }], retention_days }`.
- `POST /api/v1/projects/{id}/restore`: back to the live list (200, project summary).
- Client: `hostedProjects.trash()`, `.restore(id)`, `.deletePermanently(id)`.

## Purge
- The hourly retention sweep in `bin/hosted.rs` now also calls `ControlStore::sweep_expired_projects(cutoff)`. Projects trashed more than `AUTH_RETENTION_DAYS` (default 30) ago lose their catalog row (file and transformation rows cascade), and `WorkspaceRegistry::purge` removes `users/<user>/projects/<project>/`.
- A permanent delete does the same at once (`ControlStore::purge_project`, then the `after_project_delete` middleware removes the folder).
- Storage: trashed projects still count toward the quota until purged. A purge takes the project's live file and definition bytes off `storage_quotas`; workspace bytes drop when the folder is removed.
- If removing the folder fails after the row is gone, the files are orphaned (no route reaches them) but still counted by the workspace usage scan. This is logged as an error.

## Tests
- `control-postgres`: `trashed_projects_restore_and_purge` (DB-backed).
- `api`: catalog test extended with trash, restore, permanent delete and cross-account 404; workspace test checks files kept on trash and removed on permanent delete; `only_project_deletes_are_followed_up`.
- Web: `apps/web/src/shell/HostedProjectsPage.test.tsx`.
- DB-backed tests skip without `DATABASE_URL`, which home2 does not set, so they were compiled but not run.
