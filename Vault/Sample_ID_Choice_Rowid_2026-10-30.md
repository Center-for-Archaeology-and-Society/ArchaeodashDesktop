Up: [[Index]]

# Sample IDs choice: visible ID or rowid (2026-10-30)

Combined groups from different imports can have different visible ID columns (e.g. `anid` in one, automatic row IDs in another) or none, so procedure labels came out mixed or blank. One shared **Sample IDs** setting now picks the ID every analysis page shows and exports.

## Behaviour
- Picker next to Dataset on Explore, Visualize, Ordination, Cluster, Probabilities and Euclidean Distance. Options: Automatic (default), Sample ID (visible column name), rowid. One module-level store, kept in localStorage (`archaeodash.sampleIds`), so all pages follow one choice.
- Automatic uses rowid when the selected groups use different visible ID columns or a whole group has no IDs; otherwise the visible ID. A few missing IDs only show a note (those rows show their rowid).
- rowid is the 1-based source row number of the import, so it repeats across imports. When any rowid repeats among the combined rows, every label becomes `group:rowid`; a label still repeated (rows of two imports merged into one group) gets ` (2)`, ` (3)`.
- Covers cluster result tables, dendrogram/partition labels, assignment lists, ordination score tables and hover, Visualize hover/point labels/selected rows, Explore row labels and exports.
- Probabilities/Euclidean: the ID column field adds rowid (backend `id_column` = the legacy rowid column name) and follows the shared choice; picking the visible ID or rowid there updates the shared choice.

## Backend / contracts
- `IdColumnChoice` (`visible` | `rowid`, default visible) as optional `id_column` on the measured-data, transformed and PCA-score export requests (Rust and TS). Rowid exports start with `rowid`, then the visible ID column.
- `crates/application/src/sample_ids.rs` holds the rowid label rule; `apps/web/src/ids/sample-ids.ts` mirrors it. Clustering `id_values` uses it for the legacy rowid column.
