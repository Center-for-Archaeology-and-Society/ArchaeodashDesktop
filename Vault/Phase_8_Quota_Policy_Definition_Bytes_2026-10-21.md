# Quota Policy Revision 2: Definition Bytes (2026-10-21)

Closes the "count definition bytes in `storage_quotas`" item left open by
[[Phase_8_Hosted_Transformations_2026-10-08]] (Section 6.9: quota counts
metadata and definitions according to published policy).

## Policy

- Revision 1: `logical_bytes` = live uploaded file bytes only.
- Revision 2: `logical_bytes` = live file bytes **plus** live
  transformation-definition bytes. `file_count` still counts files only.
- One limit (`AUTH_QUOTA_BYTES`) covers both.

## Mechanics

- **Save** (`POST /api/v1/projects/{id}/transformations`): after validation
  and the project ownership check, reserve the full serialized body size,
  write the object, upsert the catalog row, then `Commit` the new bytes. On
  object-write or catalog failure the reservation is `Release`d. Over the
  limit → 413 `quota_exceeded`, with no object, row, or reservation left.
- **Replace**: the store returns `previous_bytes`, read under the same
  `FOR UPDATE` row lock as the replace, and the route `Remove`s them after
  the commit. Net charge = new size − old size. The full new size is
  reserved up front because both objects briefly coexist on disk, so a user
  right at the limit cannot replace a definition until they free space.
- **Delete**: `soft_delete_transformation` now returns
  `TombstonedTransformation { object_key, bytes }` and the route refunds
  `bytes`. A repeated delete is 404 and refunds nothing.
- **Sweep**: unchanged. Bytes leave the quota at tombstone time (same as
  files), so purging trash needs no reconciliation.

## Migration `0008_quota_policy_definitions.sql`

Backfills definitions saved under revision 1: creates quota rows for owners
who only have definitions, adds each owner's live definition bytes once
(guarded by `policy_revision < 2`), marks every row revision 2, and makes 2
the column default.

## Tests

- Store: `previous_bytes` on replace, `bytes` on tombstone; backfill test
  (two live + one tombstoned definition on a 100-byte revision-1 row → 184,
  replay unchanged).
- HTTP: `transformation_bytes_count_toward_the_quota` — create, larger
  replace, second definition, delete + repeated delete, then 413 at a tight
  limit with no row/object/reservation residue.

## Verification (2026-10-21)

- Host toolchain 1.98.1 against a live PostgreSQL 16 container: all 69
  tests in `archaeodash-control-postgres` (14) and `archaeodash-api` (55)
  pass, including the three new ones. `cargo fmt --check` clean; gate-form
  clippy (`-D warnings`, no `--all-targets`) clean on both crates. No other
  crate depends on them.
- The full `scripts/verify-local.sh` gate did **not** run: its cached image
  had been pruned, and the rebuild stalled on apt downloads.
- Dev-host quirks found: the repo mount is `noexec` (run the script with
  `bash`, set `CARGO_TARGET_DIR` outside the repo), and host→container
  traffic through published ports and the bridge IP times out. A
  `--network host` Postgres on a spare port works.

## Known gap (pre-existing, not changed here)

Project soft-delete does not refund the bytes of the project's files or
definitions; they stay charged until something reconciles them. Worth a
follow-up together with the project-namespace purge.

## Related

- [[Phase_8_Hosted_Transformations_2026-10-08]]
- [[Phase_8_Hosted_File_Catalog_2026-10-04]]
