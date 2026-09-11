# IMPLEMENTATION Section 6 - Storage architecture

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 6. Storage architecture

### 6.1 Common storage traits

Define application-facing traits rather than conditional logic throughout handlers:

- `ProjectRepository`: open, validate, migrate, and atomically publish project metadata, selection state, transformation definitions, and recovery journals.
- `ProjectScanner`: recursively discover `.parquet` candidates inside the project, exclude reserved internal directories, read footer metadata cheaply, and report readiness without loading full tables.
- `GroupFileStore`: validate/read/stage/publish/copy/move/archive self-describing group Parquet files and their bounded revision history. Implementations target a local project directory or private hosted object namespace.
- `UserFileStore`: read/write/list user-chosen source spreadsheets and explicit exports inside the project boundary; it does not impose a `sources/` location.
- `HostedControlRepository`: hosted users, sessions, ownership/catalog rows, active revision pointers, preferences, jobs, quotas, deletion state, and audit events. Analytical-unit rows never enter this repository.
- `CacheStore`: disposable Arrow/computation cache keyed by ordered group checksums, transformation definition, seed, and engine version.
- `TransactionManager`: atomic multi-group publication with a recovery journal and validation gates.
- `Clock`, `IdGenerator`, `RandomSource`, `PasswordHasher`, `EmailSender` for deterministic tests.
- `ProjectAuthorization`: assert owner/read/write/delete access on resolved project and group IDs immediately before every action.

Use the Apache Arrow `object_store` interface, or a thin compatible wrapper where application-specific semantics are needed, so the same contracts can target local files and hosted object storage. Do not leak backend paths, bucket names, or presigned storage credentials into domain objects.

### 6.2 Authoritative-data boundary

The application has four deliberately separate storage classes:

1. **Group Parquet files (authoritative working data):** one logical group per file, containing hidden `analytical_uuid`, visible ANID, original measured elemental values, descriptive fields, and embedded ArchaeoDash metadata. These files are open, portable, and directly exchangeable between projects.
2. **Optional source spreadsheets (provenance inputs):** CSV/XLSX/etc. may remain anywhere inside the project. The application records a project-relative path and checksum when available but neither duplicates the file into `sources/` nor requires it to remain present after group creation.
3. **Project metadata and history:** versioned JSON under `.archaeodash/` records manifest/index hints, import recipes, transformation definitions and seeds, transactions, recovery state, and revision history. It never determines candidate eligibility and never substitutes modified elemental values for the values in a group file.
4. **Computational state:** Arrow batches, transformed matrices, permutation samples/results, models, projections, and plots are memory/job state or disposable caches. They can always be deleted and recomputed from group files plus definitions.

The measured elemental columns of an ArchaeoDash group file are immutable after import. No log/log10 value, standardization, normalization, ratio, imputation, permutation, PCA/UMAP/LDA coordinate, cluster input, or other calculated elemental value may be persisted into a group Parquet file. Correct a measured value in the upstream source and re-import it as a new group revision; do not offer ordinary in-app cell editing for elemental columns.

### 6.3 Desktop project and file behavior

The user opens a directory as both project and workspace. ArchaeoDash does not require group files or source spreadsheets to occupy special user-visible subdirectories:

```text
MyProject/
├── excavation-2024.xlsx             # optional source; may be anywhere in project
├── study-a/
│   └── reference-group.parquet      # metadata-complete candidate group
├── my-groups/
│   ├── Group_1.parquet
│   └── Group_2.parquet
├── exports/                         # optional explicit result exports
└── .archaeodash/
    ├── project.json                 # index hints/settings, never an eligibility gate
    ├── transformations/             # definitions and seeds; no transformed data
    ├── history/                     # bounded prior group revisions
    ├── transactions/                # multi-file intent/commit records
    ├── cache/                       # disposable; excluded from discovery
    └── recovery/                    # interrupted-publication recovery
```

- Recursively scan regular `.parquet` files below the canonicalized project root, excluding `.archaeodash/cache`, `history`, `transactions`, `recovery`, staging files, explicit export-result files, symlinks that resolve outside the root, and hidden temporary names.
- A cheap footer check classifies each candidate as `ready_metadata_present`, `not_an_archaeodash_group`, `unsupported_profile`, or `unreadable`. Metadata presence makes the UI show **Ready to add**; it is not the full validation result.
- Adding/loading a ready candidate always performs full schema, metadata, path, checksum, row, identity, type, role, unit, and provenance validation. Only the successfully validated revision enters the active analysis selection. Validation is repeated when its size, modification time, ETag, or checksum changes.
- The manifest remembers validated paths/checksums and selections for speed, but deleting or failing to register a manifest entry cannot make an otherwise valid in-project group ineligible. Conversely, a stale manifest entry cannot bypass validation.
- Source import begins with a file picker limited to files inside the project. Record its relative path/checksum and import recipe; do not copy, rename, move, or require it under `sources/`. A file outside the project must first be placed within the project by the user.
- Native dialogs return user-selected paths to dedicated Rust commands. Rust may manipulate project-contained paths the user explicitly chooses, while webview JavaScript receives neither unrestricted filesystem primitives nor blanket directory enumeration.
- Group-file actions include validate/add, unload, reveal, rename/move within project, create, duplicate, import/copy from another project, export, archive/delete, inspect metadata/provenance, enable/disable editing, move/copy analytical units, split, and merge.
- Before any move/rename/delete, resolve the exact target, show it for confirmation where destructive, and use the recovery/history policy. A project-relative path change does not alter `GroupId`.
- Project metadata writes use write-to-temp, flush/fsync where supported, atomic rename, and parent-directory sync where meaningful. Group transactions use the stricter multi-file protocol in section 6.8.
- An optional SQLite or embedded index may live in the operating system application-cache directory to accelerate recent-project search. It is disposable, rebuildable, and never the source of truth for project contents.
- Maintain recent-project paths in app configuration, but never store analytical-unit contents or auth tokens in browser local storage.
- Allow portable `.adash.zip` export/import containing selected group files, optional user-selected sources, project metadata, schema versions, and checksums with Zip Slip protections; omit caches and ephemeral analysis state.
- Desktop account/login is absent by default. Local OS file permissions are the boundary. A future sync feature requires a separate design and must not silently upload local data.

### 6.4 Hosted user file store

Hosted users receive a logical private file/project namespace backed by `UserFileStore`:

```text
users/<opaque-user-id>/
└── projects/<project-id>/
    ├── files/<opaque-file-id>/<version>       # group/source/export objects; logical paths live in catalog
    ├── transformations/<transformation-id>/<revision-id>.json
    ├── history/<group-id>/<revision-id>.parquet
    ├── transactions/<transaction-id>.json
    ├── results/<result-id>/<version>          # only explicit retained/exported results
    └── cache/<input-checksum>/<engine-version>
```

- Development/single-host uses an atomic local filesystem store on a dedicated persistent volume.
- Scalable production uses a private S3-compatible bucket with server-side encryption, object versioning, lifecycle policy, and no public ACLs.
- Object keys use generated IDs, never usernames, email addresses, uploaded path components, or raw display filenames. Store display names in the authorized catalog.
- Preserve uploaded files byte-for-byte while quarantined. Capture SHA-256, detected media type, extension, byte size, parser version, upload time, and storage-native version/ETag.
- A source spreadsheet may use any logical path inside the hosted project. A group Parquet upload is published only after profile validation; afterward it behaves exactly like a desktop group file.
- Canonical group Parquet is distinct from disposable Parquet/Arrow analysis caches. Cache objects never advertise the ArchaeoDash group-file metadata profile and are excluded from discovery.
- Do not depend on object-store custom metadata as the application catalog. It is too constrained and can require copying an object to update. Store only non-sensitive integrity/operational hints there.
- Hosted file downloads stream through an authorized application route or use a narrowly scoped, short-lived signed URL only after an ownership check. Never issue bucket-list or prefix-wide credentials.

The hosted server must not expose a literal operating-system home directory or WebDAV-like arbitrary path API. “User file store” is an application-authorized namespace over opaque objects.

### 6.5 Hosted PostgreSQL control plane

Remove MySQL entirely. Retain PostgreSQL only for control-plane state that requires indexed lookup, transactions, authentication, authorization, quotas, or coordination. No imported dataset row or dataframe column is stored in PostgreSQL.

Do not recreate a physical SQL table per user/dataset/transformation. The existing naming, truncation, prefix-discovery, and cross-user authorization risks disappear when all ownership is expressed by foreign keys.

All tables include created/updated timestamps where applicable. Foreign keys use restrictive deletes unless a documented cascade is safe.

| Table | Essential columns and constraints |
|---|---|
| `users` | `id`, `username`, `username_normalized UNIQUE`, `email`, `email_normalized UNIQUE`, `password_hash`, `email_verified_at`, `disabled_at` |
| `sessions` | `id`, `user_id`, `token_hash UNIQUE`, `created_at`, `last_seen_at`, `expires_at`, `revoked_at`, optional device label; never store raw token |
| `account_tokens` | `id`, `user_id`, `kind`, `token_hash UNIQUE`, `expires_at`, `used_at`, `revoked_at`; replaces two near-identical token tables |
| `auth_throttles` | privacy-minimized keyed digest, action, window start, count, blocked-until; persistent across processes |
| `projects` | `id`, `owner_id`, display name, active manifest object/version, revision, storage bytes, deleted_at; unique active normalized name per owner |
| `files` | `id`, `project_id`, logical relative path, kind (`group`, `source`, `export`, `internal`), display filename, opaque object key, storage version/ETag, SHA-256, media type, extension, bytes, state, deleted_at; unique active logical path per project |
| `groups` | `id`, `project_id`, current file ID/path, display name, profile/schema version, active revision ID, editability, provenance summary, row/column counts, content checksum, validation state/time, deleted_at |
| `group_revisions` | `id`, `group_id`, parent ID, file ID, reason, actor, content checksum, validation-report digest, publication state |
| `group_revision_files` | transaction/revision ID, group ID, old/new file ID and checksum, role, ordinal; constrains atomic multi-group publication |
| `transformations` | `id`, owner/project scope, name, revision, definition file ID, ordered input group checksums, deleted_at |
| `analysis_results` | only explicit retained/exported results: `id`, owner/project ID, type, file ID, input group checksums, transformation revision, algorithm/version/config digest, seed, dimensions |
| `preferences` | `user_id`, key, typed JSON value; primary key `(user_id,key)` |
| `jobs` | `id`, owner, kind, status, progress, stage, input/config JSON, result/error code, timestamps, cancellation flag |
| `storage_quotas` | owner, logical/physical bytes, file/project counts, reserved bytes, limit policy revision |
| `audit_events` | actor, action, object IDs, result, request correlation ID, timestamp; no raw data values or secrets |

Keep detailed schema/role/provenance in group Parquet metadata and transformation/transaction payloads in versioned project files. PostgreSQL may copy small summary fields needed for listing, authorization, quota enforcement, and conflict detection, but a project can be exported and reconstructed from its files without database row-data tables. Use SQLx migrations as an explicit deployment step. API startup performs a read-only schema compatibility check and fails readiness if required migrations are absent; it does not execute ad hoc DDL in user sessions.

### 6.6 ArchaeoDash Group Parquet Profile

Publish the profile as a versioned, open specification with JSON Schema examples, valid/invalid fixtures, and compatibility rules. It uses standard Apache Parquet and namespaced key/value file metadata; no proprietary container is introduced. See the [Apache Parquet format specification](https://github.com/apache/parquet-format) and the Rust [Parquet crate](https://docs.rs/parquet/latest/parquet/).

Required physical columns:

- `analytical_uuid`: non-null fixed 16-byte Parquet UUID (fixed-length binary), unique within the file, generated at import and hidden from ordinary UI/export surfaces. This is the single canonical physical encoding; no writer may emit both encodings. A lowercase hyphenated UUID string column is accepted only as a one-time migration input and is canonicalized to the 16-byte form on read (Section 17.1 item 8).
- the user-selected ANID column: non-null is recommended but duplicate or blank values are reported rather than used as identity;
- every user-selected elemental column: nullable `FLOAT64` by default, with the imported measured values exactly represented under the documented numeric parsing policy;
- every selected descriptive column using a supported native Parquet type. Group name need not be repeated per row because one file is exactly one group, though a legacy group column may be retained as descriptive provenance.

Required namespaced file metadata (`archaeodash.*`):

- `profile_version`, `file_kind=group`, `group_id`, `group_name`, `group_revision`, `created_at`, and `created_by_app_version`;
- a canonical JSON `column_roles` map identifying `analytical_uuid`, `anid`, every `elemental`, and every `descriptive` column;
- elemental units per column, using a documented vocabulary plus an explicit `unknown` value that produces a compatibility warning;
- provenance containing source project/study, optional citation/license/lab/method, source spreadsheet relative path and SHA-256 when retained, import timestamp/parser/recipe, original group value, and `derived_from_group_id`/revision when applicable;
- `measured_elemental_checksum`, computed over canonical ordered `(analytical_uuid, elemental-column-id, imported value/null)` tuples, so group operations can prove measured values did not change;
- schema fingerprint and content checksum rules. Because writing a checksum into the bytes it covers is recursive, define the content checksum over a canonical logical representation or keep the whole-file SHA-256 in project revision metadata.

Optional metadata is preserved during read-modify-write when possible. Unknown required profile versions fail closed. Missing required metadata means **Not ready** in discovery. Metadata-complete files show **Ready to add**, but that badge must clearly state that full validation occurs on add/load.

Full validation checks: contained canonical path; readable footer/pages; supported profile and Parquet features; required metadata/columns; unique valid `analytical_uuid`; group ID/name consistency; allowed physical/logical types; role completeness and non-overlap; numeric parse/null constraints; elemental unit compatibility; row/column/size limits; measured checksum; schema fingerprint; provenance shape; absence of persisted derived/transformed elemental roles; and duplicate group/revision conflicts. Return a structured report with errors, warnings, file path, checksum, and remediation.

When a copied file conflicts with an existing `group_id`, never silently merge or replace it. Offer: treat it as the same known revision if checksums match; import as a new revision after ancestry validation; or clone it with a new `GroupId` while retaining provenance. `analytical_uuid` values remain stable unless the user explicitly chooses **Fork analytical units as independent**, which generates new IDs and records the mapping.

### 6.7 Group editing and comparative data

- The Group Manager operates on analytical units, never “artifacts”: create/rename/duplicate/archive groups; move or copy selected analytical units; split/merge groups; inspect provenance; and import groups copied from other research.
- Imported comparative groups default to `read_only_reference`, preserving source study/project, group ID and revision, checksum, citation, license, analytical method, units, import time, and source path/file name when safe.
- Every reference group exposes **Enable editing**. The user chooses either (a) make an editable local clone with a new `GroupId` (recommended, preserving `derived_from` provenance), or (b) edit the imported group in place when permissions allow, after an explicit warning that the local file will diverge. The original provenance is never erased. A later **Set read-only** action is also available.
- Moving an analytical unit removes it from the source group and adds it to the destination in one transaction. Copying preserves the same `analytical_uuid` because it represents the same analytical unit; analyses that select overlapping UUIDs must block by default and offer explicit de-duplication, not count the row twice.
- Duplicating a group preserves analytical UUIDs and records lineage. **Fork analytical units as independent** is a distinct, explicitly named action that generates new UUIDs and a mapping table.
- Descriptive values and group metadata may be edited through a new validated Parquet revision. Elemental columns are read-only in the table. Correcting measured elemental data requires correcting the upstream spreadsheet and re-importing as a new revision, with a comparison report.
- Cluster/membership/manual reassignment changes group membership by moving analytical units between group files; it must not write assignment columns, transformed elemental values, or statistical outputs into a group file.
- Every rewrite preserves original measured values and verifies `measured_elemental_checksum` before publication. If verification fails, abort and retain the prior files.
- Every mutation is a full staged rewrite: even a 50-row manual reassignment on a 1M-row group file rebuilds, validates, and journals the complete successor file. This rewrite cost is the accepted cost model of the one-group-per-file design; there is no partial in-place mutation path. It is budgeted, not unbounded: each mutating operation's stage/validate/publish cost is measured at the Section 12 benchmark ladder sizes (10k/100k/1M rows), interactive assignment flows on groups within the tested 1M-row ceiling stay within the Section 12 interactive latency budgets, and imports above the tested ceiling fail with an explicit size-limit error instead of degrading (Section 17.1 item 5). In-file revision buffers or delta overlays are an explicit non-goal of this architecture; introducing one would change the authoritative-file model and requires a recorded ADR. Very large single tables are a documented size limitation of the group-file model, not a silently slow path.

### 6.8 Concurrency, publication, and recovery

- Every mutating request supplies `expected_revision` or `If-Match`.
- Return `409 revision_conflict` with the current group revision on a stale edit; the client offers reload and reselects analytical units by UUID where safe.
- A multi-group operation writes an intent naming every input path/revision/checksum, selected `analytical_uuid`, intended output, and inverse recovery action. It then builds every successor Parquet file in a same-filesystem staging area.
- Validate each staged file in full and enforce transaction invariants: no lost or unexpected UUIDs; expected row-count delta; destination schema/units compatibility; source plus destination multiset behavior for move/copy; unchanged measured elemental values/checksums; and valid metadata/lineage.
- Desktop fsyncs staged files and journal where supported, archives bounded prior revisions, atomically renames replacements, writes a commit marker, then cleans staging. Because multiple filesystem renames are not one atomic primitive, startup uses the journal to deterministically finish or roll back an interrupted transaction before exposing the project.
- Hosted storage uses conditional puts/ETags, stages all objects first, commits catalog pointers together in one PostgreSQL transaction, then garbage-collects unreferenced staged objects after a safety window.
- Deletes are soft initially, followed by retention-aware asynchronous file cleanup. Object-store versioning is defense in depth, not the application retention policy. The UI must state whether restoration is available.
- Startup/reopen resolves incomplete journal entries before scanning candidates. It validates active group checksums and never guesses between divergent revisions.

### 6.9 Backup, retention, and quota rules

- Desktop projects are user-controlled files. Provide a project integrity check and document that users must include the entire project directory—including group files outside conventional folders—in their backup system.
- Hosted backups cover both PostgreSQL control-plane state and every authoritative user-file object. Record a coordinated backup watermark or catalog snapshot so restore can match pointers to file versions. Object-store versioning alone is not a backup.
- Restore testing must rebuild the catalog by scanning group metadata, reauthorize every object reference, validate sample group files, reconcile histories, and compare checksums. A successful PostgreSQL restore without corresponding user files is a failed restore.
- Quota checks reserve capacity before upload/rewrite/export and reconcile after completion. Count group revisions, sources, metadata, retained results, and exports according to published policy; disposable caches have a separate operator-controlled budget and are evicted first.
- Content deduplication, if implemented, occurs behind opaque per-owner references and must not expose cross-user file existence through timing, error, or quota behavior. Encryption/key policy must remain compatible with deletion obligations.
- Soft deletion first removes discovery/access, then expires exports and caches, then deletes unreferenced history/results/files after the documented recovery window. A group revision referenced by recovery or an active transaction cannot be physically deleted.
