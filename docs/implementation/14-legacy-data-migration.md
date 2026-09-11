# IMPLEMENTATION Section 14 - Legacy data migration

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 14. Legacy data migration

### 14.1 Discovery report

Build a read-only migrator command that produces a redacted report before writing anything:

- DB server/schema version and collation/timezone;
- auth schema and indexes;
- users, duplicate normalized usernames/emails, verification states, password hash formats;
- per-user dataset base/metadata/preferences/transformation-index tables, including 32/64-character historical variants;
- orphan metadata/payload/index tables, missing transformation parts, name collisions, malformed delimiter metadata;
- row counts, columns/types/null counts, duplicate/missing legacy row IDs, available group columns/values, checksums, and total bytes;
- ownership inference confidence and anything requiring manual mapping.

Do not print credentials, raw password hashes, tokens, email addresses, or dataset row values in the report.

### 14.2 Mapping rules

- `users` maps to normalized `users`; duplicate email policy requires a manual resolution file before import, never “first match.”
- The old database does not retain the exact original uploaded bytes, original extension, workbook sheets, formulas, cell formatting, quoting, encoding, or pre-normalized headers. Migration must not claim that it reconstructed the user's original native file.
- `<username>_<dataset>` plus `_metadata` maps to one project and one profile-valid group Parquet file per distinct legacy group value. If no usable group column exists, create one named group after applying a configured/manual naming rule. Metadata `variable` records set elemental roles.
- The generated group Parquet files are the authoritative migrated working data. Optionally generate a UTF-8 CSV or XLSX provenance export for external accessibility; neither is labeled as the original upload.
- Write `migration-provenance.json` with legacy table identifiers, migration batch/tool versions, source row/column/null counts, type mapping, checksums for every generated file, elemental roles/units, group mapping, repaired UUIDs, and explicit unrecoverable-fidelity warnings.
- `<username>_preferences` maps allowlisted keys (`lastOpenedDataset`, `themePreference`) after resolving old table name to new dataset ID.
- `<username>_transformations` and payload suffix tables map to JSON definitions only. Decode unit-separator lists and pipe ratio specs with strict validation, preserve configuration/seed when known, and discard stored logged/imputed/permuted/transformed elemental matrices after validation because the new runtime recomputes them. Historical result tables may be exported as clearly labeled archival results, never group files.
- Old combined workspace transformations are not assumed valid; current runtime skipped persisted transformations for multi-dataset workspaces. Report and require explicit handling.
- Preserve legacy names as display aliases/provenance, not object keys, paths, or physical identifiers.
- Generate `analytical_uuid` for every migrated analytical unit. Where a legacy row ID is valid, use it only to make reruns idempotent and to build a migration-only old-to-new map; do not expose it as the new internal identity.

### 14.3 Migration execution and verification

1. Back up MySQL and test restore; record backup checksum outside the repo.
2. Run discovery and resolve ambiguities.
3. Export each legacy dataset into isolated staged group Parquet files using idempotent migration batch IDs; validate the complete profile and batch invariants, upload exact generated files to the hosted `GroupFileStore`, and create only ownership, catalog, revision-pointer, auth, preference, and job summaries in PostgreSQL.
4. Compare user/project/group/transformation counts; database rows/columns/nulls against each group Parquet file; group partitions; measured-element checksums; descriptive values; preferences; and sampled archival results. Verify no imported cell value exists only in PostgreSQL and no transformed/permuted elemental value entered a group file.
5. Run authenticated user acceptance on staging.
6. Rehearse rollback and rerun from a clean target.
7. At cutover, place old writes in maintenance/read-only mode, take final backup/delta, migrate, verify, switch traffic, and retain the old service read-only for the approved rollback window.
8. Destroy temporary exports and credentials according to retention policy.
