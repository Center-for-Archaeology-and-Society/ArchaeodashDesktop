//! Sample ID labels shared by analyses and exports.
//!
//! Rows carry two IDs: the visible ID (the import's ID column, e.g. `anid`)
//! and the legacy `rowid` (the 1-based row number in the import source).
//! Combined groups from different imports can use different visible ID
//! columns, or none, so callers may label rows by rowid instead. Rowids are
//! unique within one import but repeat across imports, so when any rowid
//! repeats every label becomes `group:rowid`; a label that still repeats
//! (rows of two imports in one group) gets a ` (2)`, ` (3)`… suffix. The web
//! client (`apps/web/src/ids/sample-ids.ts`) applies the same rule.

use std::collections::{HashMap, HashSet};

use archaeodash_contracts::IdColumnChoice;
use archaeodash_data_io::GroupFileData;

/// Header of the rowid ID column in exports.
pub const ROWID_HEADER: &str = "rowid";

/// Group name per row: the synthetic file-group column (the last
/// descriptive column of a combined input).
pub(crate) fn row_groups(data: &GroupFileData) -> Vec<String> {
    data.rows
        .iter()
        .map(|r| r.descriptive.last().cloned().flatten().unwrap_or_default())
        .collect()
}

/// Unambiguous rowid label per row (see the module note). A missing rowid
/// is the 1-based row position.
pub(crate) fn rowid_labels(data: &GroupFileData, groups: &[String]) -> Vec<String> {
    let base: Vec<String> = data
        .rows
        .iter()
        .enumerate()
        .map(|(index, r)| {
            r.legacy_rowid
                .clone()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| (index + 1).to_string())
        })
        .collect();
    let mut seen = HashSet::new();
    if base.iter().all(|label| seen.insert(label.as_str())) {
        return base;
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    base.iter()
        .enumerate()
        .map(|(i, rowid)| {
            let group = groups.get(i).map(String::as_str).unwrap_or("");
            let label = format!("{group}:{rowid}");
            let n = counts.entry(label.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                label
            } else {
                format!("{label} ({n})")
            }
        })
        .collect()
}

/// Leading ID columns of an export: the visible ID alone, or the rowid
/// label followed by the visible ID (so no sample ID is lost). Returns the
/// headers and each row's cells.
pub(crate) fn export_id_columns(
    data: &GroupFileData,
    choice: IdColumnChoice,
) -> (Vec<String>, Vec<Vec<Option<String>>>) {
    let visible_header = data.profile.roles.visible_id.clone();
    let visible = data.rows.iter().map(|r| r.visible.clone());
    match choice {
        IdColumnChoice::Visible => (vec![visible_header], visible.map(|v| vec![v]).collect()),
        IdColumnChoice::Rowid => {
            let rowids = rowid_labels(data, &row_groups(data));
            // Automatic-ID imports name their visible column "rowid" too.
            let visible_header = if visible_header.eq_ignore_ascii_case(ROWID_HEADER) {
                "Sample ID".to_string()
            } else {
                visible_header
            };
            let cells = rowids
                .into_iter()
                .zip(visible)
                .map(|(rowid, v)| vec![Some(rowid), v])
                .collect();
            (vec![ROWID_HEADER.to_string(), visible_header], cells)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archaeodash_data_io::{ColumnRoles, GroupProfile, GroupRow, ImportRecipe};
    use uuid::Uuid;

    /// Rows of (group, rowid, visible ID); the group is the only descriptive cell.
    fn data(visible_id: &str, rows: &[(&str, Option<&str>, Option<&str>)]) -> GroupFileData {
        GroupFileData {
            profile: GroupProfile {
                profile_version: 1,
                file_kind: "group".into(),
                group_id: "id-A".into(),
                group_name: "A".into(),
                revision_id: "rev-A".into(),
                roles: ColumnRoles {
                    identity: "analytical_uuid".into(),
                    visible_id: visible_id.into(),
                    legacy_rowid: "legacy_rowid".into(),
                    descriptive: vec!["Group".into()],
                    elemental: vec![],
                },
                row_count: rows.len(),
                source_path: None,
                source_sha256: None,
                import_recipe: ImportRecipe::default(),
                measured_elemental_checksum: String::new(),
            },
            rows: rows
                .iter()
                .map(|(group, rowid, visible)| GroupRow {
                    uuid: Uuid::now_v7(),
                    visible: visible.map(str::to_string),
                    legacy_rowid: rowid.map(str::to_string),
                    descriptive: vec![Some((*group).to_string())],
                    elemental: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn unique_rowids_stay_plain() {
        let d = data(
            "anid",
            &[
                ("A", Some("1"), None),
                ("B", Some("2"), None),
                ("B", None, None),
            ],
        );
        assert_eq!(rowid_labels(&d, &row_groups(&d)), vec!["1", "2", "3"]);
    }

    #[test]
    fn repeated_rowids_name_their_group_and_occurrence() {
        let d = data(
            "anid",
            &[
                ("A", Some("1"), None),
                ("B", Some("1"), None),
                ("B", Some("2"), None),
                ("B", Some("1"), None),
            ],
        );
        assert_eq!(
            rowid_labels(&d, &row_groups(&d)),
            vec!["A:1", "B:1", "B:2", "B:1 (2)"]
        );
    }

    #[test]
    fn export_columns_follow_the_choice() {
        let d = data(
            "anid",
            &[("A", Some("1"), Some("AN1")), ("B", Some("1"), None)],
        );
        let (headers, cells) = export_id_columns(&d, IdColumnChoice::Visible);
        assert_eq!(headers, vec!["anid"]);
        assert_eq!(cells, vec![vec![Some("AN1".into())], vec![None]]);
        let (headers, cells) = export_id_columns(&d, IdColumnChoice::Rowid);
        assert_eq!(headers, vec!["rowid", "anid"]);
        assert_eq!(
            cells,
            vec![
                vec![Some("A:1".into()), Some("AN1".into())],
                vec![Some("B:1".into()), None],
            ]
        );
        // Automatic-ID groups: the visible column is renamed, not repeated.
        let auto = data("rowid", &[("A", Some("1"), Some("1"))]);
        assert_eq!(
            export_id_columns(&auto, IdColumnChoice::Rowid).0,
            vec!["rowid", "Sample ID"]
        );
    }
}
