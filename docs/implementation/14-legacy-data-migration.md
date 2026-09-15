# IMPLEMENTATION Section 14 - Legacy identity/contact transition

> Split from `IMPLEMENTATION.md` on 2026-09-09. Scope amended on 2026-09-14: MySQL analytical data is out of scope and is never inspected, extracted, reconciled, or migrated. Return to the [master document](../../IMPLEMENTATION.md) for the governing scope decision and cutover gates.

## 14. Legacy identity/contact transition and analytical-data non-migration

### 14.1 Explicit exclusions

- The new runtime has no MySQL dependency.
- Do not connect to MySQL for analytical-data discovery, table inventory, exports, row counts, source reconstruction, transformation recovery, preferences, result tables, or cutover reconciliation.
- Do not copy any analytical rows, elemental values, source spreadsheets, project data, transformations, results, plots, or general user preferences into PostgreSQL.
- MySQL backups and retention remain an old-service operational concern. They are not inputs to the new application, and no backup checksum or credential belongs in this repository.

### 14.2 Optional minimal account transition

Only if an authorized product decision requires retaining legacy accounts, transition the minimum necessary identity/contact state through a one-time, audited process:

1. Obtain a sanitized, authorized export containing only the account identifier, normalized contact email, verification state, and a migration-safe password-reset requirement. Never export raw passwords, password hashes, raw session/remember/reset/verification tokens, analytical data, or user preferences.
2. Create new opaque account IDs in PostgreSQL. Store the minimum identity/contact fields plus server-side authentication-security records defined in Section 11.
3. Require every transitioned account to establish a new password through an expiring reset/verification flow. Do not transplant legacy password or token material.
4. Reconcile only authorized account identifiers/counts and contact-verification outcomes. Report aggregated counts; do not place personal data in migration reports or source control.
5. Delete the authorized transition export under the published retention procedure and record only a non-sensitive completion attestation.

If no account transition is approved, new hosted accounts begin at registration and no legacy database content is imported.

### 14.3 Cutover and rollback

1. Rehearse the registration, verification, login, password-reset, logout, remembered-session revocation, rate-limit, and account-contact flows against PostgreSQL.
2. Verify that PostgreSQL contains no analytical/dataframe/source/result/project content and that file-store authorization does not rely on MySQL identifiers.
3. At cutover, switch traffic only after the auth lifecycle, backup/restore, and file-store authorization checks pass.
4. Keep the old service read-only only if required for its own retention/support obligations; it is not a data source for the new runtime.
5. Rollback restores the prior service or disables the new deployment. It never requires importing new file-store content into MySQL.
