# Excel import and TSV fix (2026-10-22)

Up: [[Index]]

## Problem
- TSV sources were read with the comma reader, so a `.tsv` file loaded as one wide column (and its upload parse-check "passed" anyway).
- XLSX uploads were accepted but never parsed (`parse_state = deferred`), so they could not be imported.

## Change
- `crates/data-io`: `SourceFormat` (csv/tsv/xlsx by extension) and `load_source(path, format, sheet)`. CSV/TSV use the text reader with the right delimiter; XLSX goes through Calamine (`excel.rs`). All three produce the same all-text frame, then the usual `dataLoader` steps (clean names, `rowid`).
- XLSX cell text follows readxl + `as.character`: numbers via `r_format_double` (`104.0` → `104`), booleans `TRUE`/`FALSE`, dates `YYYY-MM-DD[ HH:MM:SS]`, empty cells and formula errors missing. The first row is the header; blank header cells are named `...N`; fully empty rows are dropped.
- Worksheet choice: `sheet` on the import preview/commit requests (default: first non-empty sheet); the preview returns `sheet` and `sheets`. The web Import wizard shows a Worksheet picker when a workbook has more than one sheet and passes the sheet to every preview and the commit.
- Upload parse-checks (desktop `SourceFileService` and hosted `HostedFileStore`) now parse CSV, TSV, and XLSX with the explicit format.

## Verification
- Rust (on home2, target dir outside the noexec mount): data-io `excel.rs` tests (first non-empty sheet, named sheet, non-workbook rejected) and `tsv_sources_split_on_tabs`; application `xlsx_and_tsv_sources_preview_and_commit` and the files upload test (truncated vs real XLSX, TSV). All tests in data-io, contracts, application, desktop, and api pass; `cargo fmt --check` and gate clippy (`-D warnings`, no `--all-targets`) clean.
- Web: contracts/client/web typecheck clean; 128/128 web tests pass.
- Not done: desktop app rebuild and a manual import of a real workbook in the app.

Related: [[Interaction_Log_2026-10-22]]
