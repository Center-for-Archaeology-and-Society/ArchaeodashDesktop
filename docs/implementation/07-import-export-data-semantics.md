# IMPLEMENTATION Section 7 - Import, export, and data semantics

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 7. Import, export, and data semantics

### 7.1 Supported formats

Initial spreadsheet imports: CSV and TSV through a dedicated text reader with an explicit dialect/encoding policy, and XLSX, XLS, XLSB, and ODS where Calamine coverage passes fixtures. Calamine is a binary-spreadsheet reader only; it is not the CSV/TSV path and CSV/TSV fixture gates do not depend on it. A conforming ArchaeoDash Group Parquet file is loaded directly rather than passed through spreadsheet import. Initial exports: CSV, TSV, XLSX, standard group Parquet, result files, and a portable ArchaeoDash project bundle.

The current undocumented `rio::import`/`rio::export` catch-all is deliberately dropped. It makes accepted formats environment-dependent and expands parser risk. When an actual user format is identified, add it through a format-specific adapter and fixture rather than reopening arbitrary detection.

Publish a capability matrix in the application and Help content. Distinguish `read as source`, `create group files`, `group-file editing`, and `result export`; these are different promises:

| Format | Read as import source | Directly eligible for analysis | Writable purpose | Full source-structure round trip |
|---|---|---|---|---|
| CSV/TSV | Yes | No; import creates group Parquet | explicit data/result export | No byte-level promise |
| XLSX | Yes | No; import creates group Parquet | new workbook/sheet export | No unless separately proven |
| XLS/XLSB/ODS | Yes when fixture-gated | No; import creates group Parquet | XLSX/CSV/TSV export | No |
| ArchaeoDash Group Parquet | Yes | Yes after full validation | group operations and group export | Profile/schema fidelity required |
| Other Parquet | Inspectable | No until required profile metadata/schema are supplied by a controlled conversion | generic export only | Schema/metadata fixture gate |
| Arrow IPC | Optional source/cache | No | disposable transport/cache | Schema/metadata fixture gate |

Never describe a format as editable merely because it is readable. Unsupported same-format writeback must be visible before the user begins editing.

### 7.2 Import contract

1. Select a spreadsheet already located anywhere inside the opened project. Canonicalize the path, reject escapes, stream its SHA-256, and record the project-relative path, display filename, extension, media type, size, and modification/storage version. Do not copy it to `sources/` or any other required location.
2. Validate extension, MIME signature/magic, compressed/uncompressed size, sheet count, row count, column count, per-cell size, duplicate headers, and formula/error cells before creating groups.
3. Default limit starts at the current 100 MiB request ceiling but must also cap expanded workbook size, rows, columns, total cells, and processing time. Limits are configuration with safe server maxima; desktop can allow higher values with a warning.
4. Parse to a typed intermediate table while retaining original text and null distinctions needed for preview. Parsing never mutates the source spreadsheet.
5. Ask the user to identify the ANID column, optional group column, elemental columns, and descriptive columns. If there is no group column, require a new group name and put every imported analytical unit in that group.
6. Normalize headers with a documented compatibility function matching `janitor::clean_names(case = "none")` for legacy imports. Resolve collisions deterministically and show the mapping before confirmation; store original and normalized names in group-file metadata.
7. Remove an incoming `analytical_uuid` or `rowid` from identity duties unless it came from a fully validated group file. Preserve it as a renamed descriptive legacy column if requested and generate a fresh immutable `analytical_uuid` for each imported analytical unit.
8. Default ANID to the case-insensitive `ANID` match; otherwise require an explicit selection. ANID remains visible but is never the internal identity.
9. Default elemental candidates use the current INAA list (`as, la, lu, nd, sm, u, yb, ce, co, cr, cs, eu, fe, hf, ni, rb, sb, sc, sr, ta, tb, th, zn, zr, al, ba, ca, dy, k, mn, na, ti, v`); if none match, offer numeric-like columns excluding identity, ANID, group, and chosen descriptive columns.
10. Numeric-like inference samples the first 1,500 rows and requires at least a 95% parse rate, preserving the current optimized behavior. Display parse failures before import.
11. Treat zero/negative/null import policies as interpretation into the original-measurement group columns and record the exact recipe. Preview every value-class change and never later replace those stored values with analysis transformations.
12. Partition rows by the chosen group value using deterministic, filename-safe names plus stable `GroupId`s. Blank group values require an explicit destination choice. Stage one group Parquet file per resulting group.
13. Each output receives the complete required profile metadata, hidden analytical UUIDs, ANID, selected elemental/descriptive fields, source path/checksum, source row mapping, import recipe, and measured-element checksum. Do not store the original group field redundantly unless the user retains it as descriptive provenance.
14. Validate every staged group file and the batch invariants before publishing any file. On name collisions, offer rename, verified replacement/new revision, or cancellation; never silently overwrite.
15. For append, choose a target group explicitly, validate compatible roles/units/types, align the union of descriptive columns, generate UUIDs for new source rows, and create a new group revision. Do not coerce or modify existing measured elemental values.

### 7.3 Export contract

- Export selector includes original measured chemical data, an explicitly computed transformed result, PCA scores, membership probabilities, and other supported results, fixing the current `pcaData`/`pcadf` mismatch.
- The extension and explicit format selector must agree; reject unknown extensions instead of silently emitting XLSX.
- Web returns a short-lived authenticated download stream; desktop opens a native save dialog and Rust writes directly to that exact user-approved path.
- Source files remain ordinary project files and can be revealed/downloaded directly when present. `Save project` persists settings/definitions; `Export result as…` writes a separate result; `Export group…` emits a conforming group Parquet file.
- Default exports to a new filename. Replacing an existing desktop file or hosted export requires explicit confirmation and produces recoverable version history where supported.
- CSV/TSV exports protect spreadsheet users from formula injection by default for string cells beginning with `=`, `+`, `-`, or `@`; provide a clearly labeled raw-data option if scientific workflows require exact text.
- XLSX uses `rust_xlsxwriter`; binary spreadsheet imports (XLSX/XLS/XLSB/ODS) use `calamine`; CSV/TSV imports use the dedicated text reader from Section 7.1. Polars remains the dataframe engine.
- Do not present an exported XLSX as the original workbook: formulas, macros, charts, formatting, hidden content, and external links are not recreated unless explicitly implemented and tested.
- Never export calculated elemental values back into a group Parquet file. When a user requests logged, imputed, permuted, normalized, or ratio-derived values, label the output as an analysis-result export without `file_kind=group` metadata.
- Plot export preserves filename, width, height, and DPI controls. Support PNG first and SVG/PDF only when the chosen rendering path has deterministic tests. Do not claim an extension works until tested.
