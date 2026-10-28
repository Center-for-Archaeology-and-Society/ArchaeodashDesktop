//! Combined multi-file analysis input: one request `path` plus zero or more
//! `extra_paths`, read as a single virtual group file.
//!
//! Every analysis and export request that accepts `extra_paths` reads its
//! input through [`CombinedInput`], so they all agree on row order, column
//! sets, and the synthetic file-group column:
//!
//! - rows: `path`'s rows, then each extra path's rows, in request order, each
//!   file's rows in file order (the `GroupService::rows` order);
//! - elemental columns: the first file's elemental columns that exist (as
//!   elemental columns) in every file, in first-file order;
//! - descriptive columns: the union of every file's descriptive columns in
//!   first-seen order (cells absent from a file are `None`), then one
//!   synthetic descriptive column holding each row's file `group_name`
//!   (see [`synthetic_group_column`]).
//!
//! The synthetic column is present for single-file reads too, so it can
//! always be named as a group/color column. The virtual profile is the first
//! file's profile with the combined roles and row count; it is read-only.

use std::collections::HashSet;
use std::path::Path;

use archaeodash_contracts::TransformationDefinition;
use archaeodash_data_io::{read_group_file, GroupFileData, GroupRow};
use archaeodash_domain::DomainError;

/// Preferred name of the synthetic file-group column.
pub const SYNTHETIC_GROUP_COLUMN: &str = "Group";
/// Synthetic column name used when an input already has a real `"Group"`
/// descriptive column.
pub const SYNTHETIC_GROUP_COLUMN_FALLBACK: &str = "Group (file)";

/// Synthetic file-group column name for a set of inputs: `"Group"`, unless
/// any input has a real descriptive column named exactly `"Group"`, in which
/// case `"Group (file)"`. Pass the descriptive column names of every input.
pub fn synthetic_group_column<'a>(
    mut descriptive_names: impl Iterator<Item = &'a str>,
) -> &'static str {
    if descriptive_names.any(|name| name == SYNTHETIC_GROUP_COLUMN) {
        SYNTHETIC_GROUP_COLUMN_FALLBACK
    } else {
        SYNTHETIC_GROUP_COLUMN
    }
}

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

fn import_err(e: archaeodash_data_io::ImportError) -> DomainError {
    DomainError::Internal(Box::new(e))
}

/// Runs `read` while holding the project's shared lock. Group changes
/// (moves, assignments, renames) publish under the exclusive lock, so the
/// read waits for a change being saved instead of reading a half-published
/// set of files or keeping a file open that the change must replace (which
/// makes the save fail with "Access is denied" on Windows). The lock is
/// released before returning.
pub(crate) fn read_locked<T>(
    root: &Path,
    read: impl FnOnce() -> Result<T, DomainError>,
) -> Result<T, DomainError> {
    let _shared = archaeodash_file_store_fs::lock_project_shared(root)
        .map_err(|e| DomainError::Internal(Box::new(e)))?;
    read()
}

/// Per-file metadata kept for validation messages.
#[derive(Debug, Clone)]
pub struct CombinedSource {
    pub path: String,
    pub group_name: String,
    pub row_count: usize,
    pub elemental: Vec<String>,
    pub descriptive: Vec<String>,
}

/// One virtual group file assembled from `path` plus `extra_paths`.
#[derive(Debug, Clone)]
pub struct CombinedInput {
    /// Combined rows and the read-only virtual profile; the last descriptive
    /// column is the synthetic file-group column.
    pub data: GroupFileData,
    /// Source files in request order.
    pub sources: Vec<CombinedSource>,
    /// Name of the synthetic file-group column (last descriptive column).
    pub group_column: &'static str,
}

impl CombinedInput {
    /// Reads `path` then each `extra_paths` entry from a local project root,
    /// under the shared project lock (see [`read_locked`]).
    pub fn read_local(
        root: &Path,
        path: &str,
        extra_paths: &[String],
    ) -> Result<Self, DomainError> {
        read_locked(root, || {
            Self::read_with(path, extra_paths, |rel| {
                read_group_file(&root.join(rel)).map_err(import_err)
            })
        })
    }

    /// Reads `path` then each `extra_paths` entry with a caller-supplied
    /// reader (which applies its own path safety and resource checks).
    /// A path listed twice is rejected with `duplicate_input_path`.
    pub fn read_with(
        path: &str,
        extra_paths: &[String],
        mut read: impl FnMut(&str) -> Result<GroupFileData, DomainError>,
    ) -> Result<Self, DomainError> {
        let mut seen = HashSet::new();
        let mut files = Vec::with_capacity(1 + extra_paths.len());
        for rel in std::iter::once(path).chain(extra_paths.iter().map(String::as_str)) {
            if !seen.insert(rel) {
                return Err(validation(
                    "duplicate_input_path",
                    format!("{rel}: group file is listed more than once"),
                ));
            }
            files.push((rel.to_string(), read(rel)?));
        }
        Self::from_files(files)
    }

    /// Combines already-loaded files (request order; at least one file).
    pub fn from_files(files: Vec<(String, GroupFileData)>) -> Result<Self, DomainError> {
        let group_column = synthetic_group_column(
            files
                .iter()
                .flat_map(|(_, data)| data.profile.roles.descriptive.iter().map(String::as_str)),
        );
        let Some((_, first)) = files.first() else {
            return Err(validation("input_empty", "no group files"));
        };

        let elemental: Vec<String> = first
            .profile
            .roles
            .elemental
            .iter()
            .filter(|name| {
                files
                    .iter()
                    .all(|(_, data)| data.profile.roles.elemental.contains(name))
            })
            .cloned()
            .collect();
        let mut descriptive: Vec<String> = Vec::new();
        for (_, data) in &files {
            for name in &data.profile.roles.descriptive {
                if !descriptive.contains(name) {
                    descriptive.push(name.clone());
                }
            }
        }

        let mut profile = first.profile.clone();
        let total_rows: usize = files.iter().map(|(_, data)| data.rows.len()).sum();
        let mut rows = Vec::with_capacity(total_rows);
        let mut sources = Vec::with_capacity(files.len());
        for (rel, data) in files {
            let roles = &data.profile.roles;
            let elemental_idx: Vec<usize> = elemental
                .iter()
                .map(|name| {
                    roles
                        .elemental
                        .iter()
                        .position(|c| c == name)
                        .unwrap_or_default()
                })
                .collect();
            let descriptive_idx: Vec<Option<usize>> = descriptive
                .iter()
                .map(|name| roles.descriptive.iter().position(|c| c == name))
                .collect();
            let group_name = data.profile.group_name.clone();
            for row in &data.rows {
                let mut cells: Vec<Option<String>> = descriptive_idx
                    .iter()
                    .map(|idx| idx.and_then(|i| row.descriptive.get(i).cloned().flatten()))
                    .collect();
                cells.push(Some(group_name.clone()));
                rows.push(GroupRow {
                    uuid: row.uuid,
                    visible: row.visible.clone(),
                    legacy_rowid: row.legacy_rowid.clone(),
                    descriptive: cells,
                    elemental: elemental_idx
                        .iter()
                        .map(|&i| row.elemental.get(i).copied().flatten())
                        .collect(),
                });
            }
            sources.push(CombinedSource {
                path: rel,
                group_name,
                row_count: data.rows.len(),
                elemental: roles.elemental.clone(),
                descriptive: roles.descriptive.clone(),
            });
        }

        if sources.len() > 1 {
            profile.group_name = sources
                .iter()
                .map(|s| s.group_name.as_str())
                .collect::<Vec<_>>()
                .join(" + ");
        }
        profile.row_count = rows.len();
        profile.roles.elemental = elemental;
        descriptive.push(group_column.to_string());
        profile.roles.descriptive = descriptive;

        Ok(Self {
            data: GroupFileData { profile, rows },
            sources,
            group_column,
        })
    }

    /// True when more than one file was combined.
    pub fn is_multi(&self) -> bool {
        self.sources.len() > 1
    }

    /// Error-message context: the request path, plus a count of extra files.
    pub fn context(&self) -> String {
        match self.sources.len() {
            0 | 1 => self
                .sources
                .first()
                .map(|s| s.path.clone())
                .unwrap_or_default(),
            n => format!("{} (+{} more)", self.sources[0].path, n - 1),
        }
    }

    /// Real descriptive columns (everything except the synthetic column).
    pub fn real_descriptive(&self) -> &[String] {
        let all = &self.data.profile.roles.descriptive;
        &all[..all.len().saturating_sub(1)]
    }

    /// Requires every named input column to exist in every file. A name that
    /// is an elemental column in any file must be elemental in every file
    /// (`missing_elemental_column`); a name that is descriptive in any file
    /// must be descriptive in every file (`missing_group_column`). The
    /// synthetic column, each file's visible-ID / legacy-rowid column, and
    /// names present in no file (e.g. post-transform outputs) are left to
    /// the downstream checks. Errors name the offending file and column.
    pub fn require_columns<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), DomainError> {
        if !self.is_multi() {
            return Ok(());
        }
        for name in names {
            if name == self.group_column {
                continue;
            }
            if self
                .sources
                .iter()
                .any(|s| s.elemental.iter().any(|c| c == name))
            {
                if let Some(missing) = self
                    .sources
                    .iter()
                    .find(|s| !s.elemental.iter().any(|c| c == name))
                {
                    return Err(validation(
                        "missing_elemental_column",
                        format!(
                            "{}: column {name:?} is not a measured elemental column",
                            missing.path
                        ),
                    ));
                }
            } else if self
                .sources
                .iter()
                .any(|s| s.descriptive.iter().any(|c| c == name))
            {
                if let Some(missing) = self
                    .sources
                    .iter()
                    .find(|s| !s.descriptive.iter().any(|c| c == name))
                {
                    return Err(validation(
                        "missing_group_column",
                        format!(
                            "{}: column {name:?} is not a descriptive column",
                            missing.path
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// [`Self::require_columns`] over analysis columns plus the raw columns a
    /// transformation definition reads (its elemental and ratio columns).
    pub fn require_inputs(
        &self,
        columns: &[String],
        transformation: Option<&TransformationDefinition>,
    ) -> Result<(), DomainError> {
        let mut names: Vec<&str> = columns.iter().map(String::as_str).collect();
        if let Some(definition) = transformation {
            names.extend(definition.elemental_columns.iter().map(String::as_str));
            for spec in &definition.ratios {
                names.push(spec.numerator.as_str());
                names.push(spec.denominator.as_str());
            }
        }
        self.require_columns(names)
    }

    /// Group labels from the synthetic column, in combined row order.
    pub fn file_group_labels(&self) -> Vec<String> {
        self.sources
            .iter()
            .flat_map(|s| std::iter::repeat_n(s.group_name.clone(), s.row_count))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_data_io::{ColumnRoles, GroupProfile, ImportRecipe};
    use uuid::Uuid;

    type TestRow<'a> = (&'a str, Vec<Option<&'a str>>, Vec<Option<f64>>);

    fn file(
        group: &str,
        descriptive: &[&str],
        elemental: &[&str],
        rows: &[TestRow],
    ) -> GroupFileData {
        GroupFileData {
            profile: GroupProfile {
                profile_version: 1,
                file_kind: "group".into(),
                group_id: format!("id-{group}"),
                group_name: group.into(),
                revision_id: format!("rev-{group}"),
                roles: ColumnRoles {
                    identity: "analytical_uuid".into(),
                    visible_id: "anid".into(),
                    legacy_rowid: "legacy_rowid".into(),
                    descriptive: descriptive.iter().map(|s| s.to_string()).collect(),
                    elemental: elemental.iter().map(|s| s.to_string()).collect(),
                },
                row_count: rows.len(),
                source_path: None,
                source_sha256: None,
                import_recipe: ImportRecipe::default(),
                measured_elemental_checksum: String::new(),
            },
            rows: rows
                .iter()
                .map(|(id, d, e)| GroupRow {
                    uuid: Uuid::now_v7(),
                    visible: Some(id.to_string()),
                    legacy_rowid: Some(id.to_string()),
                    descriptive: d.iter().map(|v| v.map(str::to_string)).collect(),
                    elemental: e.clone(),
                })
                .collect(),
        }
    }

    fn two_files() -> Vec<(String, GroupFileData)> {
        vec![
            (
                "groups/A.parquet".into(),
                file(
                    "A",
                    &["Site", "Type"],
                    &["as", "fe", "zn"],
                    &[
                        (
                            "a1",
                            vec![Some("s1"), Some("t1")],
                            vec![Some(1.0), Some(2.0), Some(3.0)],
                        ),
                        (
                            "a2",
                            vec![Some("s2"), None],
                            vec![Some(4.0), None, Some(6.0)],
                        ),
                    ],
                ),
            ),
            (
                "groups/B.parquet".into(),
                file(
                    "B",
                    &["Ware", "Site"],
                    &["zn", "as", "cu"],
                    &[(
                        "b1",
                        vec![Some("w1"), Some("s3")],
                        vec![Some(30.0), Some(10.0), Some(5.0)],
                    )],
                ),
            ),
        ]
    }

    #[test]
    fn synthetic_name_rule() {
        assert_eq!(
            synthetic_group_column(["Site", "Type"].into_iter()),
            "Group"
        );
        assert_eq!(synthetic_group_column(std::iter::empty()), "Group");
        assert_eq!(
            synthetic_group_column(["Site", "Group"].into_iter()),
            "Group (file)"
        );
        // Exact match only.
        assert_eq!(
            synthetic_group_column(["group", "Groups"].into_iter()),
            "Group"
        );
    }

    #[test]
    fn combines_rows_columns_and_synthetic_group() {
        let files = two_files();
        let uuids: Vec<Uuid> = files
            .iter()
            .flat_map(|(_, d)| d.rows.iter().map(|r| r.uuid))
            .collect();
        let combined = CombinedInput::from_files(files).expect("combine");
        let data = &combined.data;
        assert!(combined.is_multi());
        assert_eq!(combined.group_column, "Group");
        // Intersection in first-file order.
        assert_eq!(data.profile.roles.elemental, vec!["as", "zn"]);
        // Union in first-seen order, then the synthetic column.
        assert_eq!(
            data.profile.roles.descriptive,
            vec!["Site", "Type", "Ware", "Group"]
        );
        assert_eq!(combined.real_descriptive(), ["Site", "Type", "Ware"]);
        assert_eq!(data.profile.row_count, 3);
        assert_eq!(data.profile.revision_id, "rev-A");
        let ids: Vec<_> = data
            .rows
            .iter()
            .map(|r| r.visible.clone().expect("id"))
            .collect();
        assert_eq!(ids, vec!["a1", "a2", "b1"]);
        assert_eq!(data.rows.iter().map(|r| r.uuid).collect::<Vec<_>>(), uuids);
        assert_eq!(data.rows[1].elemental, vec![Some(4.0), Some(6.0)]);
        assert_eq!(data.rows[2].elemental, vec![Some(10.0), Some(30.0)]);
        assert_eq!(
            data.rows[2].descriptive,
            vec![Some("s3".into()), None, Some("w1".into()), Some("B".into())]
        );
        assert_eq!(
            data.rows[0].descriptive,
            vec![Some("s1".into()), Some("t1".into()), None, Some("A".into())]
        );
        assert_eq!(combined.file_group_labels(), vec!["A", "A", "B"]);
        assert_eq!(combined.context(), "groups/A.parquet (+1 more)");

        // Requested columns must exist in every file, naming the file.
        combined
            .require_columns(["as", "zn", "Site", "Group", "PC1"])
            .expect("shared columns");
        let err = combined
            .require_columns(["fe"])
            .expect_err("fe missing in B");
        assert!(err.to_string().contains("missing_elemental_column"));
        assert!(err.to_string().contains("groups/B.parquet"), "{err}");
        let err = combined
            .require_columns(["Type"])
            .expect_err("Type missing in B");
        assert!(err.to_string().contains("missing_group_column"));
        assert!(err.to_string().contains("\"Type\""), "{err}");
    }

    #[test]
    fn single_file_keeps_columns_and_adds_synthetic() {
        let mut files = two_files();
        files.truncate(1);
        let combined = CombinedInput::from_files(files).expect("combine");
        assert!(!combined.is_multi());
        assert_eq!(combined.data.profile.group_name, "A");
        assert_eq!(
            combined.data.profile.roles.elemental,
            vec!["as", "fe", "zn"]
        );
        assert_eq!(
            combined.data.profile.roles.descriptive,
            vec!["Site", "Type", "Group"]
        );
        assert_eq!(
            combined.data.rows[1].elemental,
            vec![Some(4.0), None, Some(6.0)]
        );
        assert_eq!(combined.context(), "groups/A.parquet");
    }

    #[test]
    fn real_group_column_renames_synthetic() {
        let mut files = two_files();
        files[1].1.profile.roles.descriptive[0] = "Group".into();
        let combined = CombinedInput::from_files(files).expect("combine");
        assert_eq!(combined.group_column, "Group (file)");
        assert_eq!(
            combined.data.profile.roles.descriptive,
            vec!["Site", "Type", "Group", "Group (file)"]
        );
        assert_eq!(
            combined.data.rows[2].descriptive[2..],
            [Some("w1".to_string()), Some("B".to_string())]
        );
    }

    #[test]
    fn duplicate_path_is_rejected() {
        let files = two_files();
        let a = files[0].1.clone();
        let err = CombinedInput::read_with(
            "groups/A.parquet",
            &["groups/A.parquet".to_string()],
            |_| Ok(a.clone()),
        )
        .expect_err("duplicate");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "duplicate_input_path"
        ));
    }

    #[test]
    fn read_local_waits_while_a_change_is_being_saved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        for (rel, data) in two_files() {
            let path = root.join(&rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            archaeodash_data_io::write_group_rows(&path, data.profile, &data.rows)
                .expect("write group");
        }
        // A group change holds the exclusive project lock while it publishes.
        let store = archaeodash_file_store_fs::FsGroupFileStore::new(&root).expect("store");
        let saving = store.lock_exclusive().expect("exclusive lock");
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let reader_root = root.clone();
        let reader = std::thread::spawn(move || {
            let input = CombinedInput::read_local(
                &reader_root,
                "groups/A.parquet",
                &["groups/B.parquet".to_string()],
            );
            let _ = done_tx.send(());
            input
        });
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(300))
                .is_err(),
            "the read finished while a change was being saved"
        );
        drop(saving);
        done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the read finishes once the change is saved");
        let input = reader
            .join()
            .expect("reader thread")
            .expect("combined input");
        assert_eq!(input.sources.len(), 2);
    }
}
