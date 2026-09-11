# IMPLEMENTATION Section 5 - Shared domain model

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 5. Shared domain model

### 5.1 Identity types

Use opaque IDs at every boundary:

- `UserId`, `SessionId`, `ProjectId`, `GroupId`, `GroupRevisionId`, `TransformationId`, `AnalysisResultId`, `JobId`, and `AnalyticalUuid`.
- Use UUIDv7 or ULID for sortable, collision-resistant IDs.
- The physical Parquet column is named `analytical_uuid`. It is immutable across filtering, transformations, ordinations, clustering, membership, distance outputs, group moves/copies, and exports.
- `analytical_uuid` is an internal technical identity. Never show it as a normal column, picker option, label, tooltip, or exported user-facing identifier. Diagnostics may include a redacted/copyable value only in an explicit advanced support view.
- Preserve a source file's `rowid` as an ordinary `legacy_rowid` descriptive column when useful, but generate a new `analytical_uuid`. Never use dataframe position or the user-visible ANID as technical identity.
- Each loaded analytical unit also carries `SourceRef { group_id, group_revision_id, source_path, source_row }`, replacing `currentDatasetRowMap` and temporary `.__source_*` columns.

### 5.2 Core entities

`Project`

- opened directory or hosted namespace, project ID/name, settings, remembered group paths, transaction history, and schema version;
- the project root is the hard inclusion boundary for analysis; filesystem paths are canonicalized before use and may not escape it through `..`, symlinks, archive entries, or object-store keys;
- the project manifest accelerates discovery, remembers selections and history, and records revisions, but it does not decide whether a Parquet file is eligible.

`GroupFile`

- one self-describing Parquet file containing exactly one logical group of analytical units;
- relative project path, `GroupId`, group name, revision, content checksum, row count, schema/profile versions, editability mode, provenance, and validation state;
- imported measured elemental columns are immutable values; descriptive columns and group-file metadata may change through a validated rewrite;
- a group file may be located anywhere under the project root other than reserved internal transaction/cache/history locations.

`GroupRevision`

- immutable revision metadata for a published group file: parent revision, reason, actor/mode, old/new checksums, affected analytical UUIDs, source/destination paths, timestamp, and validation report;
- reasons include import, create group, move analytical units, copy analytical units, duplicate group, descriptive edit, metadata edit, reference edit enablement, merge, split, and legacy migration;
- the active Parquet file is replaced atomically after its staged successor passes full validation. Bounded history/recovery copies may be retained under `.archaeodash/`.

`Workspace`

- discovered candidate files, validated/loaded groups, combined analytical-unit/source map, active schema, selected groups, selected predictors, and unsaved UI context;
- only fully validated, currently loaded group files inside the open project contribute rows to an analysis;
- ephemeral by default; persist selections only when needed for crash recovery or resumption.

`TransformationDefinition`

- name; ordered group file checksums; included group IDs; selected metadata/predictor fields; ratio specs and mode; import missing-value flags; imputation method/config/seed; transform method; permutation configuration/seed; PCA/UMAP/LDA flags/config/seeds; visualization preferences; created/updated timestamps;
- definitions and seeds are durable JSON metadata, but calculated elemental values and analysis matrices are not. A definition is rerun from measured values whenever an analysis is requested.

`AnalysisResult`

- type (`selected`, `selected_all`, `pca_scores`, `pca_model`, `umap_scores`, `lda_scores`, `lda_model`, `cluster`, `membership`, `euclidean`, `plot_spec`);
- input group checksums, transformation ID/revision, algorithm/version/config, seed, and job lifecycle;
- ephemeral by default. A user may explicitly export a result to a separate file, but it must never be written into or mistaken for an ArchaeoDash group file.

### 5.3 Explicit state instead of `reactiveValues`

The current global `rvals` bus must be decomposed as follows:

| Current field family | New owner |
|---|---|
| `data`, `importedData`, `selectedData`, `selectedDataAll` | validated group-file view or ephemeral workspace/analysis matrix keyed by group checksums |
| `currentDatasetName`, `currentDatasetKey`, `currentDatasetRowMap`, `tbls` | project scanner, group catalog, and workspace service |
| `chem`, `initialChem`, `attr`, `attrs`, `attrGroups`, `attrGroupsSub` | transformation draft with schema IDs, not raw strings |
| import flags | import recipe persisted in project metadata and group-file provenance |
| ratio/impute/transform/run flags | transformation definition |
| `pca`, `pcadf`, `umapdf`, `LDAmod`, `LDAdf` | typed ephemeral analysis results |
| `plotdf`, `plotVars`, `plotVarChoices`, plot settings | client visualization state derived from results |
| `brushSelected` | client selection store of hidden `AnalyticalUuid`s |
| `clusterDT`, `clusterPlot`, `membershipProbs`, `edistance`, `multiplot` | ephemeral result/job outputs or explicit exports |
| merge/pending-overwrite fields | short-lived command/dialog state |
| `runPCAx`, `runUMAPx`, `runLDAx` | delete; jobs have explicit states |
| `df` | delete; belongs only to disconnected legacy subset code |
| `pcaData` | delete erroneous alias; PCA export uses `pca_scores` |
| `credentials.*` | server session/auth principal; desktop local principal |

Client state should be split between TanStack Query (server/IPC resource state) and a small Zustand store (draft selections, layout, checked rows, dialog state). Every long action uses an explicit state machine: `idle -> validating -> queued -> running -> succeeded|failed|cancelled|timed_out`.
