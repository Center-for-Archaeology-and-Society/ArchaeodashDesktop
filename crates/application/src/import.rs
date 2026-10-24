//! Import use cases shared by the HTTP and Tauri adapters (Section 10.2,
//! Phase 2 local form): preview a source spreadsheet and commit one validated
//! group Parquet file per group value, all paths contained in the project.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use archaeodash_contracts::{
    CommittedGroup, ExistingImportGroup, ImportCommitOptions, ImportCommitRequest,
    ImportCommitResponse, ImportMode, ImportPreviewRequest, ImportPreviewResponse,
    PartitionPreview,
};
use archaeodash_data_io::{
    build_group_rows, default_chem_columns_for_frame, default_id_column, load_source,
    parse_r_numeric, partition_by_group, sanitize_group_name, write_group_rows, CandidateStatus,
    ColumnRoles, GroupFileData, GroupProfile, GroupRow, ImportError, ImportRecipe, LoadedSource,
    Partition, ScanCandidate, SourceFormat, TextFrame, PROFILE_VERSION,
};
use archaeodash_file_store_fs::FsGroupFileStore;
use archaeodash_storage::{
    FileInput, GroupFileStore, PlannedOutput, StoreError, Transaction, TransactionAction,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Default project-relative destination for first-import group files.
const DEFAULT_DESTINATION_DIR: &str = "groups";

/// Initial revision for a first import (later revisions go through the
/// transactional group store, not this use case).
const INITIAL_REVISION: &str = "rev-1";

/// Visible-ID column used for automatic IDs: the 1-based source row number
/// that `dataLoader` prepends to every frame.
pub const AUTOMATIC_ID_COLUMN: &str = "rowid";

fn io_err(e: std::io::Error) -> ImportError {
    ImportError::Io(e.to_string())
}

/// Resolves the two mutually exclusive import grouping modes. No grouping is
/// valid for a metadata-only preview; commits must always select a mode.
fn import_partitions(
    frame: &archaeodash_data_io::TextFrame,
    group_column: Option<&str>,
    group_name: Option<&str>,
    required: bool,
) -> Result<Vec<Partition>, ImportError> {
    let column = group_column.filter(|column| !column.trim().is_empty());
    let name = group_name.filter(|name| !name.trim().is_empty());
    if group_name.is_some() && name.is_none() {
        return Err(ImportError::Parse("group name cannot be blank".into()));
    }
    match (column, name) {
        (Some(_), Some(_)) => Err(ImportError::Parse(
            "choose either a group column or one group name, not both".into(),
        )),
        (Some(column), None) => partition_by_group(frame, column),
        (None, Some(name)) => Ok(vec![Partition {
            group_name: name.trim().to_string(),
            row_indices: (0..frame.n_rows()).collect(),
        }]),
        (None, None) if required => Err(ImportError::Parse(
            "choose a group column or provide one group name".into(),
        )),
        (None, None) => Ok(Vec::new()),
    }
}

/// Visible-ID role column for a commit. An omitted choice keeps the legacy
/// default (`anid` when present); an explicit empty choice or `rowid` means
/// automatic IDs, which use the source row numbers like the legacy loader's
/// "Use rowid (default)" option.
fn resolve_visible_id(requested: Option<&str>, frame: &archaeodash_data_io::TextFrame) -> String {
    match requested.map(str::trim) {
        Some("") | Some(AUTOMATIC_ID_COLUMN) => AUTOMATIC_ID_COLUMN.to_string(),
        Some(column) => column.to_string(),
        None => match default_id_column(&frame.columns) {
            Some(column) => column,
            None => "anid".to_string(),
        },
    }
}

/// An ID column as the wizard names it.
fn id_column_label(column: &str) -> String {
    if column == AUTOMATIC_ID_COLUMN {
        "automatic IDs (row numbers)".to_string()
    } else {
        column.to_string()
    }
}

fn store_err(e: StoreError) -> ImportError {
    match e {
        StoreError::Io(message) => ImportError::Io(message),
        other => ImportError::Parse(other.to_string()),
    }
}

/// SHA-256 hex digest of a file's bytes.
fn sha256_file(path: &Path) -> Result<String, ImportError> {
    let bytes = std::fs::read(path).map_err(io_err)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// A destination folder as a `/`-separated relative path without outer
/// slashes or blanks.
fn normalize_folder(folder: &str) -> String {
    folder
        .replace('\\', "/")
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// The folder of a project-relative `/` path (`""` for the project root).
fn folder_of(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

fn file_name_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// `rev-N` bumps to `rev-(N+1)`; any other format gets a fresh UUIDv7.
fn next_revision(current: &str) -> String {
    current
        .strip_prefix("rev-")
        .and_then(|n| n.parse::<u64>().ok())
        .map(|n| format!("rev-{}", n + 1))
        .unwrap_or_else(|| Uuid::now_v7().simple().to_string())
}

fn input_of(path: &str, data: &GroupFileData) -> FileInput {
    FileInput {
        path: path.to_string(),
        revision_id: data.profile.revision_id.clone(),
        measured_elemental_checksum: data.profile.measured_elemental_checksum.clone(),
    }
}

/// Reorders each row's descriptive cells from `from` column order to `to`
/// (a superset); columns the row lacks become empty, like `bind_rows`.
fn align_rows(rows: Vec<GroupRow>, from: &[String], to: &[String]) -> Vec<GroupRow> {
    if from == to {
        return rows;
    }
    let index: Vec<Option<usize>> = to
        .iter()
        .map(|column| from.iter().position(|c| c == column))
        .collect();
    rows.into_iter()
        .map(|mut row| {
            row.descriptive = index
                .iter()
                .map(|i| i.and_then(|i| row.descriptive[i].clone()))
                .collect();
            row
        })
        .collect()
}

/// Lineage label of an imported source: the project-relative path, plus
/// `#sheet` for a workbook so imports of different worksheets of one file
/// are told apart.
fn source_label(source: &str, sheet: Option<&str>) -> String {
    match sheet {
        Some(sheet) if is_workbook(source) => format!("{source}#{sheet}"),
        _ => source.to_string(),
    }
}

fn is_workbook(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".xlsx")
}

/// The worksheet recorded in a lineage label (`None` for text sources and
/// for labels written before worksheets were recorded). The sheet follows
/// the first `.xlsx#`, so a worksheet name may itself contain `#`.
fn sheet_of_label(label: &str) -> Option<&str> {
    const MARK: &str = ".xlsx#";
    // ASCII lowercasing keeps byte offsets, so `at` indexes `label` too.
    let at = label.to_ascii_lowercase().find(MARK)?;
    Some(&label[at + MARK.len()..])
}

/// Whether a group was imported from the same source as `sha256`/`sheet`:
/// same bytes and, when both record one, the same worksheet.
fn same_source(profile: &GroupProfile, sha256: &str, sheet: Option<&str>) -> bool {
    if profile.source_sha256.as_deref() != Some(sha256) {
        return false;
    }
    let recorded = profile.source_path.as_deref().and_then(sheet_of_label);
    match (recorded, sheet) {
        (Some(recorded), Some(sheet)) => recorded == sheet,
        _ => true,
    }
}

/// Everything one commit decided about the source, shared by the modes.
struct ImportPlan<'a> {
    frame: &'a TextFrame,
    partitions: Vec<Partition>,
    visible_id_column: String,
    elemental_columns: Vec<String>,
    descriptive_columns: Option<Vec<String>>,
    recipe: ImportRecipe,
    destination_dir: String,
    /// Lineage label (`source_label`).
    source: String,
    source_sha256: String,
    sheet: Option<String>,
}

impl ImportPlan<'_> {
    fn is_same_source(&self, profile: &GroupProfile) -> bool {
        same_source(profile, &self.source_sha256, self.sheet.as_deref())
    }

    /// `folder/id.parquet`, or `folder/id_N.parquet` for `n > 1`.
    fn path_for(&self, group_id: &str, n: usize) -> String {
        let file = if n > 1 {
            format!("{group_id}_{n}.parquet")
        } else {
            format!("{group_id}.parquet")
        };
        if self.destination_dir.is_empty() {
            file
        } else {
            format!("{}/{file}", self.destination_dir)
        }
    }

    fn build(
        &self,
        partition: &Partition,
        elemental: &[String],
    ) -> Result<(ColumnRoles, Vec<GroupRow>), ImportError> {
        build_group_rows(
            self.frame,
            partition,
            &self.visible_id_column,
            elemental,
            self.descriptive_columns.as_deref(),
            &self.recipe,
            None,
        )
    }

    fn profile(
        &self,
        group_id: &str,
        group_name: &str,
        revision: &str,
        roles: ColumnRoles,
    ) -> GroupProfile {
        GroupProfile {
            profile_version: PROFILE_VERSION,
            file_kind: "group".to_string(),
            group_id: group_id.to_string(),
            group_name: group_name.to_string(),
            revision_id: revision.to_string(),
            roles,
            row_count: 0,
            source_path: Some(self.source.clone()),
            source_sha256: Some(self.source_sha256.clone()),
            import_recipe: self.recipe.clone(),
            measured_elemental_checksum: String::new(),
        }
    }

    fn output(
        &self,
        path: String,
        group_id: &str,
        group_name: &str,
        revision: String,
        roles: ColumnRoles,
        rows: Vec<GroupRow>,
    ) -> PlannedOutput {
        PlannedOutput {
            path,
            group_id: group_id.to_string(),
            group_name: group_name.to_string(),
            revision_id: revision,
            rows,
            roles,
            recipe: self.recipe.clone(),
            source_path: Some(self.source.clone()),
            source_sha256: Some(self.source_sha256.clone()),
        }
    }

    /// How many rows of an existing group cannot have come from this source.
    /// The bytes are the same, so a row of this source is the frame row at
    /// its `legacy_rowid` and rebuilds, with the group's own ID column,
    /// measured columns, and recipe, to the same visible ID and measured
    /// values. Rows appended from another file or moved in from another
    /// group fail that test.
    fn foreign_rows(&self, data: &GroupFileData) -> usize {
        let rowid = self.frame.column_index(AUTOMATIC_ID_COLUMN);
        let mut candidates: Vec<(usize, usize)> = Vec::new();
        for (index, row) in data.rows.iter().enumerate() {
            let Some(label) = row.legacy_rowid.as_deref() else {
                continue;
            };
            let in_frame = |at: usize| {
                at < self.frame.n_rows()
                    && rowid.and_then(|c| self.frame.rows[at][c].as_deref()) == Some(label)
            };
            let at = label
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .filter(|&at| in_frame(at));
            if let Some(at) = at {
                candidates.push((index, at));
            }
        }
        if candidates.is_empty() {
            return data.rows.len();
        }
        let roles = &data.profile.roles;
        let partition = Partition {
            group_name: data.profile.group_name.clone(),
            row_indices: candidates.iter().map(|&(_, at)| at).collect(),
        };
        let Ok((_, rebuilt)) = build_group_rows(
            self.frame,
            &partition,
            &roles.visible_id,
            &roles.elemental,
            Some(&[]),
            &data.profile.import_recipe,
            None,
        ) else {
            return data.rows.len();
        };
        let same_value = |a: &Option<f64>, b: &Option<f64>| match (a, b) {
            (Some(a), Some(b)) => a == b || (a.is_nan() && b.is_nan()),
            (None, None) => true,
            _ => false,
        };
        let own = candidates
            .iter()
            .zip(&rebuilt)
            .filter(|((index, _), new)| {
                let old = &data.rows[*index];
                old.visible == new.visible
                    && old.elemental.len() == new.elemental.len()
                    && old
                        .elemental
                        .iter()
                        .zip(&new.elemental)
                        .all(|(a, b)| same_value(a, b))
            })
            .count();
        data.rows.len() - own
    }

    /// The measured columns for rows added to an existing group: exactly the
    /// group's own (in its order). A column the group measures but the user
    /// left unchecked is taken as measured when its values are numeric
    /// (the legacy type harmonization); anything else is refused.
    fn harmonized_elemental(
        &self,
        partition: &Partition,
        target: &GroupFileData,
    ) -> Result<Vec<String>, ImportError> {
        let wanted = &target.profile.roles.elemental;
        let name = &target.profile.group_name;
        let extra: Vec<&str> = self
            .elemental_columns
            .iter()
            .filter(|c| !wanted.contains(c))
            .map(String::as_str)
            .collect();
        if !extra.is_empty() {
            return Err(ImportError::Parse(format!(
                "\u{201c}{name}\u{201d} does not measure {}; add these rows as new groups \
                 in another folder, or leave those columns unchecked",
                extra.join(", ")
            )));
        }
        for column in wanted {
            if self.elemental_columns.contains(column) {
                continue;
            }
            let Some(index) = self.frame.column_index(column) else {
                return Err(ImportError::Parse(format!(
                    "\u{201c}{name}\u{201d} measures {column}, which this file does not have"
                )));
            };
            let numeric = partition.row_indices.iter().all(|&row| {
                let cell = self.frame.rows[row][index].as_deref().unwrap_or("").trim();
                cell.is_empty() || parse_r_numeric(cell).is_some_and(|v| !v.is_nan())
            });
            if !numeric || column == &self.visible_id_column {
                return Err(ImportError::Parse(format!(
                    "\u{201c}{name}\u{201d} measures {column}, but this file's {column} values \
                     are not all numbers"
                )));
            }
        }
        Ok(wanted.clone())
    }
}

/// Shared import use cases rooted at one local project directory.
pub struct ImportService {
    root: PathBuf,
    store: FsGroupFileStore,
}

impl ImportService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ImportError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        let store = FsGroupFileStore::new(&root).map_err(|e| ImportError::Io(e.to_string()))?;
        Ok(Self { root, store })
    }

    /// Lexically contains the project root: rejects absolute paths, `..`,
    /// and root/prefix components (Section 6.3 path-containment rule).
    fn resolve(&self, rel: &str) -> Result<PathBuf, ImportError> {
        let mut out = self.root.clone();
        for component in Path::new(rel).components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                other => {
                    return Err(ImportError::Parse(format!(
                        "path {rel:?} escapes the project boundary: {other:?}"
                    )))
                }
            }
        }
        Ok(out)
    }

    /// Parses the source with the `dataLoader` semantics and returns the
    /// default role suggestions plus the optional group partition summary.
    /// Never modifies the source (Section 10.2 preview contract).
    pub fn preview(
        &self,
        req: &ImportPreviewRequest,
    ) -> Result<ImportPreviewResponse, ImportError> {
        let loaded = self.load(&req.source, req.sheet.as_deref())?;
        let frame = loaded.frame;
        let id_column = default_id_column(&frame.columns);
        let elemental_columns = default_chem_columns_for_frame(&frame);
        let partitions: Vec<PartitionPreview> = import_partitions(
            &frame,
            req.group_column.as_deref(),
            req.group_name.as_deref(),
            false,
        )?
        .into_iter()
        .map(|partition| PartitionPreview {
            suggested_path: format!(
                "{DEFAULT_DESTINATION_DIR}/{}.parquet",
                sanitize_group_name(&partition.group_name)
            ),
            row_count: partition.row_indices.len() as u64,
            group_name: partition.group_name,
        })
        .collect();
        let source_sha256 = sha256_file(&self.resolve(&req.source)?)?;
        let sheet = loaded.sheet.as_deref();
        let existing_groups = self
            .ready_groups()?
            .into_iter()
            .map(|(path, profile)| ExistingImportGroup {
                folder: folder_of(&path),
                same_source: same_source(&profile, &source_sha256, sheet),
                path,
                group_name: profile.group_name,
                revision_id: profile.revision_id,
            })
            .collect();
        Ok(ImportPreviewResponse {
            source: req.source.clone(),
            row_count: frame.n_rows() as u64,
            columns: frame.columns,
            id_column,
            elemental_columns,
            partitions,
            sheet: loaded.sheet,
            sheets: loaded.sheets,
            source_sha256: Some(source_sha256),
            existing_groups,
        })
    }

    /// Loads a source by its extension (CSV, TSV, or XLSX worksheet).
    fn load(&self, source: &str, sheet: Option<&str>) -> Result<LoadedSource, ImportError> {
        let path = self.resolve(source)?;
        let format = SourceFormat::from_path(&path).ok_or_else(|| {
            ImportError::Parse(format!(
                "{source}: unsupported source format; use CSV, TSV, or XLSX"
            ))
        })?;
        load_source(&path, format, sheet)
    }

    /// Partitions the source by the group column and publishes one
    /// profile-valid Parquet file per group under the destination directory.
    /// Duplicate sanitized names get deterministic `_2`, `_3`, ... suffixes in
    /// partition order. Every published file passes full validation.
    pub fn commit(&self, req: &ImportCommitRequest) -> Result<ImportCommitResponse, ImportError> {
        self.commit_with(req, &ImportCommitOptions::default())
    }

    /// `commit` with the optional choices: which non-elemental columns stay
    /// descriptive, and what to do about groups already in the destination
    /// folder (see [`ImportMode`]; no mode keeps the `_2` suffix behavior).
    pub fn commit_with(
        &self,
        req: &ImportCommitRequest,
        options: &ImportCommitOptions,
    ) -> Result<ImportCommitResponse, ImportError> {
        let source_path = self.resolve(&req.source)?;
        let loaded = self.load(&req.source, req.sheet.as_deref())?;
        let frame = loaded.frame;

        let visible_id_column = resolve_visible_id(req.visible_id_column.as_deref(), &frame);
        let elemental_columns = req
            .elemental_columns
            .clone()
            .unwrap_or_else(|| default_chem_columns_for_frame(&frame));
        let recipe = req
            .recipe
            .clone()
            .map_or(ImportRecipe::default(), |dto| ImportRecipe {
                zero_as_na: dto.zero_as_na,
                negative_as_na: dto.negative_as_na,
                na_as_zero: dto.na_as_zero,
                blank_non_element_label: dto.blank_non_element_label,
            });
        let destination_dir = req
            .destination_dir
            .as_deref()
            .map(normalize_folder)
            .unwrap_or_else(|| DEFAULT_DESTINATION_DIR.to_string());
        // Path-containment check without touching the filesystem.
        self.resolve(&destination_dir)?;
        // An existing folder keeps its on-disk spelling, so the folder's
        // groups are found on case-insensitive file systems.
        let destination_dir = crate::groups::on_disk_folder(&self.root, &destination_dir);
        // Validate partitioning before any filesystem side effects: a commit
        // rejected for blank group values must not create directories.
        let partitions = import_partitions(
            &frame,
            Some(&req.group_column),
            req.group_name.as_deref(),
            true,
        )?;
        if options.mode.is_some() && elemental_columns.is_empty() {
            return Err(ImportError::Parse(
                "choose at least one measured column".into(),
            ));
        }
        let source_sha256 = sha256_file(&source_path)?;
        let plan = ImportPlan {
            frame: &frame,
            partitions,
            visible_id_column,
            elemental_columns,
            descriptive_columns: options.descriptive_columns.clone(),
            recipe,
            destination_dir,
            source: source_label(&req.source, loaded.sheet.as_deref()),
            source_sha256,
            sheet: loaded.sheet,
        };
        let groups = match options.mode {
            None => self.publish_new(&plan, true)?,
            Some(ImportMode::NewGroups) => {
                self.refuse_existing(&plan)?;
                self.publish_new(&plan, false)?
            }
            Some(ImportMode::ReplaceSource) => self.replace_source(&plan)?,
            Some(ImportMode::Append) => self.append_rows(&plan)?,
        };
        Ok(ImportCommitResponse {
            source: req.source.clone(),
            groups,
        })
    }

    /// Ready group files in the project with project-relative `/` paths.
    fn ready_groups(&self) -> Result<Vec<(String, GroupProfile)>, ImportError> {
        let base = self.store.resolve("").map_err(store_err)?;
        let mut out = Vec::new();
        for candidate in self.scan()? {
            if let CandidateStatus::ReadyToAdd(profile) = candidate.status {
                let rel = candidate
                    .path
                    .strip_prefix(&base)
                    .or_else(|_| candidate.path.strip_prefix(&self.root))
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .map_err(|_| {
                        ImportError::Io(format!(
                            "group file {} is outside the project",
                            candidate.path.display()
                        ))
                    })?;
                out.push((rel, *profile));
            }
        }
        Ok(out)
    }

    /// Ready groups directly in one folder.
    fn groups_in_folder(&self, folder: &str) -> Result<Vec<(String, GroupProfile)>, ImportError> {
        Ok(self
            .ready_groups()?
            .into_iter()
            .filter(|(path, _)| folder_of(path) == folder)
            .collect())
    }

    /// `NewGroups` refuses a folder that already has the planned file names
    /// or groups imported from the same source (re-importing would hold the
    /// same samples twice under new identities).
    fn refuse_existing(&self, plan: &ImportPlan<'_>) -> Result<(), ImportError> {
        let existing = self.groups_in_folder(&plan.destination_dir)?;
        let same_source: Vec<&str> = existing
            .iter()
            .filter(|(_, profile)| plan.is_same_source(profile))
            .map(|(_, profile)| profile.group_name.as_str())
            .collect();
        if !same_source.is_empty() {
            return Err(ImportError::Parse(format!(
                "folder \u{201c}{}\u{201d} already holds {} imported from this file ({}). \
                 Replace them, add the rows to them, or import into another folder",
                plan.destination_dir,
                plural(same_source.len(), "group"),
                same_source.join(", ")
            )));
        }
        let names: HashSet<String> = existing.iter().map(|(p, _)| file_name_of(p)).collect();
        let clashes: Vec<String> = plan
            .partitions
            .iter()
            .map(|p| format!("{}.parquet", sanitize_group_name(&p.group_name)))
            .filter(|name| names.contains(name))
            .collect();
        if !clashes.is_empty() {
            return Err(ImportError::Parse(format!(
                "folder \u{201c}{}\u{201d} already has group files named {}. \
                 Import into another folder, or add the rows to those groups",
                plan.destination_dir,
                clashes.join(", ")
            )));
        }
        Ok(())
    }

    /// Writes one new group file per partition (the original first-import
    /// path). `suffix_existing` keeps the legacy `_2` naming for files that
    /// already exist; otherwise an existing file is refused. Names repeated
    /// within this import always get suffixes.
    fn publish_new(
        &self,
        plan: &ImportPlan<'_>,
        suffix_existing: bool,
    ) -> Result<Vec<CommittedGroup>, ImportError> {
        // Build every group before taking the lock or creating directories,
        // so a bad column choice publishes nothing.
        let mut built = Vec::with_capacity(plan.partitions.len());
        for partition in &plan.partitions {
            built.push(plan.build(partition, &plan.elemental_columns)?);
        }
        // Coordinate destination probing and every publication with store
        // transactions and readers for this project.
        let _project_lock = self
            .store
            .lock_exclusive()
            .map_err(|e| ImportError::Io(e.to_string()))?;
        let mut planned: HashSet<String> = HashSet::new();
        let mut groups = Vec::new();
        for (partition, (roles, rows)) in plan.partitions.iter().zip(built) {
            let group_id = sanitize_group_name(&partition.group_name);
            let mut rel = plan.path_for(&group_id, 1);
            let mut suffix = 2usize;
            loop {
                let taken = planned.contains(&rel);
                if !taken && self.resolve(&rel)?.exists() && !suffix_existing {
                    return Err(ImportError::Parse(format!(
                        "folder \u{201c}{}\u{201d} already has a group file named {}",
                        plan.destination_dir,
                        file_name_of(&rel)
                    )));
                }
                if !taken && !self.resolve(&rel)?.exists() {
                    break;
                }
                rel = plan.path_for(&group_id, suffix);
                suffix += 1;
            }
            planned.insert(rel.clone());
            let profile = plan.profile(&group_id, &partition.group_name, INITIAL_REVISION, roles);
            let profile = write_group_rows(&self.resolve(&rel)?, profile, &rows)?;
            archaeodash_data_io::validate_group_file(&self.resolve(&rel)?)?;
            groups.push(CommittedGroup {
                group_id,
                group_name: partition.group_name.clone(),
                row_count: profile.row_count as u64,
                path: rel,
                revision_id: profile.revision_id,
                measured_elemental_checksum: profile.measured_elemental_checksum,
            });
        }
        Ok(groups)
    }

    /// Replaces the destination folder's groups imported from this source
    /// (same SHA-256) in one journaled transaction. Rows keep their hidden
    /// identities (matched by source row number), so copies of these samples
    /// in other folders still refer to the same samples.
    fn replace_source(&self, plan: &ImportPlan<'_>) -> Result<Vec<CommittedGroup>, ImportError> {
        let in_folder = self.groups_in_folder(&plan.destination_dir)?;
        let (replaced, others): (Vec<_>, Vec<_>) = in_folder
            .into_iter()
            .partition(|(_, profile)| plan.is_same_source(profile));
        if replaced.is_empty() {
            return Err(ImportError::Parse(format!(
                "folder \u{201c}{}\u{201d} has no groups imported from this file; \
                 import new groups instead",
                plan.destination_dir
            )));
        }
        let replaced_paths: Vec<String> = replaced.iter().map(|(p, _)| p.clone()).collect();
        let other_paths: Vec<String> = others.iter().map(|(p, _)| p.clone()).collect();
        let old = self.read_snapshot(&replaced_paths)?;
        let other_data = self.read_snapshot(&other_paths)?;
        // The replacement holds only this source's rows, so rows added to
        // these groups later (appended from another file, or moved in from
        // another group) would be dropped or take a source row's identity.
        for (_, data) in &old {
            let foreign = plan.foreign_rows(data);
            if foreign > 0 {
                return Err(ImportError::Parse(format!(
                    "\u{201c}{}\u{201d} holds {} that {} not imported from this file, and \
                     replacing it would drop {}; move {} to another group first, or import \
                     into another folder",
                    data.profile.group_name,
                    plural(foreign, "row"),
                    if foreign == 1 { "was" } else { "were" },
                    if foreign == 1 { "it" } else { "them" },
                    if foreign == 1 { "it" } else { "them" },
                )));
            }
        }

        // Same source bytes, so the same row number with the same visible ID
        // is the same sample.
        let key = |row: &GroupRow| (row.legacy_rowid.clone(), row.visible.clone());
        let mut identity: HashMap<(Option<String>, Option<String>), Uuid> = HashMap::new();
        let mut repeated = HashSet::new();
        for (_, data) in &old {
            for row in data.rows.iter().filter(|row| row.legacy_rowid.is_some()) {
                if identity.insert(key(row), row.uuid).is_some() {
                    repeated.insert(key(row));
                }
            }
        }
        for row_key in &repeated {
            identity.remove(row_key);
        }
        let elsewhere: HashMap<Uuid, &str> = other_data
            .iter()
            .flat_map(|(_, data)| {
                data.rows
                    .iter()
                    .map(|row| (row.uuid, data.profile.group_name.as_str()))
            })
            .collect();

        let replaced_set: HashSet<&str> = replaced_paths.iter().map(String::as_str).collect();
        let old_by_path: HashMap<&str, &GroupFileData> =
            old.iter().map(|(p, d)| (p.as_str(), d)).collect();
        let mut planned: HashSet<String> = HashSet::new();
        let mut outputs = Vec::new();
        for partition in &plan.partitions {
            let (roles, mut rows) = plan.build(partition, &plan.elemental_columns)?;
            for row in &mut rows {
                let kept = identity.remove(&key(row));
                if let Some(uuid) = kept {
                    if let Some(group) = elsewhere.get(&uuid) {
                        return Err(ImportError::Parse(format!(
                            "a sample from this file is now in \u{201c}{group}\u{201d}, which was \
                             not imported from it; move it back or import into another folder"
                        )));
                    }
                    row.uuid = uuid;
                }
            }
            let group_id = sanitize_group_name(&partition.group_name);
            let mut rel = plan.path_for(&group_id, 1);
            let mut suffix = 2usize;
            loop {
                if planned.contains(&rel) {
                    rel = plan.path_for(&group_id, suffix);
                    suffix += 1;
                    continue;
                }
                if !replaced_set.contains(rel.as_str()) && self.resolve(&rel)?.exists() {
                    return Err(ImportError::Parse(format!(
                        "folder \u{201c}{}\u{201d} has a group file named {} that was not \
                         imported from this file",
                        plan.destination_dir,
                        file_name_of(&rel)
                    )));
                }
                break;
            }
            planned.insert(rel.clone());
            let revision = match old_by_path.get(rel.as_str()) {
                Some(data) => next_revision(&data.profile.revision_id),
                None => INITIAL_REVISION.to_string(),
            };
            let name = partition.group_name.clone();
            outputs.push(plan.output(rel, &group_id, &name, revision, roles, rows));
        }
        let delete_paths: Vec<String> = replaced_paths
            .iter()
            .filter(|p| !planned.contains(*p))
            .cloned()
            .collect();
        let inputs = old.iter().map(|(p, d)| input_of(p, d)).collect();
        self.run_transaction(
            TransactionAction::ReplaceImport,
            inputs,
            outputs,
            delete_paths,
        )
    }

    /// Adds the rows to the destination folder's groups of the same name (new
    /// groups for the rest) in one journaled transaction, like the legacy
    /// "Add rows to existing workspace data" (`merge_loaded_data`): the
    /// folder's descriptive columns become the union of old and new ones,
    /// measured columns must match each existing group (a numeric column the
    /// group measures is taken as measured), and new rows get new identities.
    fn append_rows(&self, plan: &ImportPlan<'_>) -> Result<Vec<CommittedGroup>, ImportError> {
        let in_folder = self.groups_in_folder(&plan.destination_dir)?;
        if let Some((_, profile)) = in_folder.iter().find(|(_, p)| plan.is_same_source(p)) {
            return Err(ImportError::Parse(format!(
                "rows from this file are already in folder \u{201c}{}\u{201d} \
                 (\u{201c}{}\u{201d}); replace them instead of adding them again",
                plan.destination_dir, profile.group_name
            )));
        }
        let paths: Vec<String> = in_folder.iter().map(|(p, _)| p.clone()).collect();
        let existing = self.read_snapshot(&paths)?;
        let by_name: HashMap<&str, usize> = existing
            .iter()
            .enumerate()
            .map(|(i, (_, data))| (data.profile.group_name.as_str(), i))
            .collect();

        // Build the incoming rows of every partition with its target's
        // measured columns.
        let mut incoming = Vec::with_capacity(plan.partitions.len());
        for partition in &plan.partitions {
            let target = by_name.get(partition.group_name.as_str()).copied();
            if let Some(index) = target {
                // Rows of one group share one ID column; otherwise the new
                // IDs would land under the group's ID heading (and the
                // group's ID column would be added again as descriptive).
                let profile = &existing[index].1.profile;
                if profile.roles.visible_id != plan.visible_id_column {
                    return Err(ImportError::Parse(format!(
                        "\u{201c}{}\u{201d} uses {} as its ID column; choose the same ID \
                         column to add rows to it",
                        profile.group_name,
                        id_column_label(&profile.roles.visible_id)
                    )));
                }
            }
            let elemental = match target {
                Some(index) => plan.harmonized_elemental(partition, &existing[index].1)?,
                None => plan.elemental_columns.clone(),
            };
            let (roles, rows) = plan.build(partition, &elemental)?;
            incoming.push((target, roles, rows));
        }

        // Union of descriptive columns over the folder and the new rows.
        let mut union: Vec<String> = Vec::new();
        let existing_columns = existing
            .iter()
            .flat_map(|(_, data)| data.profile.roles.descriptive.iter());
        let incoming_columns = incoming
            .iter()
            .flat_map(|(_, roles, _)| roles.descriptive.iter());
        for column in existing_columns.chain(incoming_columns) {
            if !union.contains(column) {
                union.push(column.clone());
            }
        }
        // Every written group gets the union as its descriptive columns, so
        // none of them may use one of those names as a measured, ID, or
        // identity column (a file would hold two columns of one name).
        let written_roles = existing
            .iter()
            .map(|(_, data)| (data.profile.group_name.as_str(), &data.profile.roles))
            .chain(
                plan.partitions
                    .iter()
                    .zip(&incoming)
                    .filter(|(_, (target, _, _))| target.is_none())
                    .map(|(partition, (_, roles, _))| (partition.group_name.as_str(), roles)),
            );
        for (name, roles) in written_roles {
            let clash = union.iter().find(|column| {
                roles.elemental.contains(column)
                    || **column == roles.visible_id
                    || **column == roles.legacy_rowid
                    || **column == roles.identity
            });
            if let Some(column) = clash {
                return Err(ImportError::Parse(format!(
                    "{column} is descriptive in some groups of folder \u{201c}{}\u{201d} but \
                     measured or the ID column in \u{201c}{name}\u{201d}; use the same column \
                     roles, or import into another folder",
                    plan.destination_dir
                )));
            }
        }

        let mut appended: HashMap<usize, Vec<GroupRow>> = HashMap::new();
        let mut new_groups = Vec::new();
        for (partition, (target, roles, rows)) in plan.partitions.iter().zip(incoming) {
            let rows = align_rows(rows, &roles.descriptive, &union);
            match target {
                Some(index) => appended.entry(index).or_default().extend(rows),
                None => new_groups.push((partition, roles, rows)),
            }
        }

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        let mut reported = Vec::new();
        for (index, (path, data)) in existing.iter().enumerate() {
            let added = appended.remove(&index);
            if added.is_none() && data.profile.roles.descriptive == union {
                continue;
            }
            if added.is_some() {
                reported.push(path.clone());
            }
            let mut rows = align_rows(data.rows.clone(), &data.profile.roles.descriptive, &union);
            rows.extend(added.unwrap_or_default());
            let mut roles = data.profile.roles.clone();
            roles.descriptive = union.clone();
            inputs.push(input_of(path, data));
            outputs.push(PlannedOutput {
                path: path.clone(),
                group_id: data.profile.group_id.clone(),
                group_name: data.profile.group_name.clone(),
                revision_id: next_revision(&data.profile.revision_id),
                rows,
                roles,
                recipe: data.profile.import_recipe.clone(),
                source_path: data.profile.source_path.clone(),
                source_sha256: data.profile.source_sha256.clone(),
            });
        }
        let mut planned: HashSet<String> = HashSet::new();
        for (partition, mut roles, rows) in new_groups {
            roles.descriptive = union.clone();
            let group_id = sanitize_group_name(&partition.group_name);
            let mut rel = plan.path_for(&group_id, 1);
            let mut suffix = 2usize;
            while planned.contains(&rel) || self.resolve(&rel)?.exists() {
                rel = plan.path_for(&group_id, suffix);
                suffix += 1;
            }
            planned.insert(rel.clone());
            reported.push(rel.clone());
            let name = partition.group_name.clone();
            let revision = INITIAL_REVISION.to_string();
            outputs.push(plan.output(rel, &group_id, &name, revision, roles, rows));
        }
        let all =
            self.run_transaction(TransactionAction::AppendImport, inputs, outputs, Vec::new())?;
        // Report the groups that received rows, not realigned siblings.
        Ok(reported
            .iter()
            .filter_map(|path| all.iter().find(|g| &g.path == path).cloned())
            .collect())
    }

    fn read_snapshot(&self, paths: &[String]) -> Result<Vec<(String, GroupFileData)>, ImportError> {
        let snapshot = self.store.read_groups_snapshot(paths).map_err(store_err)?;
        paths
            .iter()
            .zip(snapshot)
            .map(|(path, data)| match data {
                Some(data) => Ok((path.clone(), data)),
                None => Err(ImportError::NotFound(format!(
                    "group file {path} disappeared"
                ))),
            })
            .collect()
    }

    /// Executes one journaled import transaction and reports every output.
    fn run_transaction(
        &self,
        action: TransactionAction,
        inputs: Vec<FileInput>,
        outputs: Vec<PlannedOutput>,
        delete_paths: Vec<String>,
    ) -> Result<Vec<CommittedGroup>, ImportError> {
        let tx = Transaction {
            action,
            selected_uuids: outputs
                .iter()
                .flat_map(|o| o.rows.iter().map(|r| r.uuid))
                .collect(),
            inputs,
            outputs,
            delete_paths,
        };
        self.store.execute(&tx).map_err(store_err)?;
        tx.outputs
            .iter()
            .map(|out| {
                let profile = archaeodash_data_io::validate_group_file(&self.resolve(&out.path)?)?;
                Ok(CommittedGroup {
                    group_id: out.group_id.clone(),
                    group_name: out.group_name.clone(),
                    path: out.path.clone(),
                    row_count: profile.row_count as u64,
                    revision_id: profile.revision_id,
                    measured_elemental_checksum: profile.measured_elemental_checksum,
                })
            })
            .collect()
    }

    /// Manifest-free discovery of published group files with their readiness
    /// states (Section 10.2 `GET /projects/{id}/candidates`, local form).
    /// Results are sorted by path; full validation happens on add/load.
    pub fn scan(&self) -> Result<Vec<ScanCandidate>, ImportError> {
        self.store
            .scan_candidates_snapshot()
            .map_err(|e| ImportError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_contracts::ImportRecipeDto;
    use archaeodash_data_io::read_group_file;

    const MINI_CSV: &str = "anid,Site,Sub-region,as,fe\n\
                            A1,Baca,Central,1.5,3\n\
                            A2,Baca,Central,2,4\n\
                            A3,Hooper,,,6\n";

    fn service_with_source() -> (ImportService, tempdir::TempDir, String) {
        let dir = tempdir::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("mini.csv"), MINI_CSV).expect("write source");
        let service = ImportService::new(dir.path()).expect("service");
        (service, dir, "mini.csv".to_string())
    }

    // Minimal tempdir shim so the dev-dependency list stays as-is.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        pub fn tempdir() -> Result<TempDir, std::io::Error> {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "archaeodash-import-test-{}-{}",
                std::process::id(),
                n
            ));
            std::fs::create_dir_all(&dir)?;
            Ok(TempDir(dir))
        }
    }

    #[test]
    fn preview_returns_defaults_and_partitions_without_touching_source() {
        let (service, _dir, source) = service_with_source();
        let req = ImportPreviewRequest {
            sheet: None,
            source: source.clone(),
            group_column: Some("Site".into()),
            group_name: None,
        };
        let resp = service.preview(&req).expect("preview");
        assert_eq!(resp.row_count, 3);
        assert_eq!(
            resp.columns,
            vec!["rowid", "anid", "Site", "Sub_region", "as", "fe"]
        );
        assert_eq!(resp.id_column.as_deref(), Some("anid"));
        // INAA-list matches in frame order.
        assert_eq!(
            resp.elemental_columns,
            vec!["as".to_string(), "fe".to_string()]
        );
        assert_eq!(resp.partitions.len(), 2);
        assert_eq!(resp.partitions[0].group_name, "Baca");
        assert_eq!(resp.partitions[0].row_count, 2);
        assert_eq!(resp.partitions[0].suggested_path, "groups/Baca.parquet");
        // Source untouched: no group files exist after a preview.
        assert!(!service.resolve("groups").expect("resolve").exists());
    }

    #[test]
    fn preview_rejects_path_escape() {
        let (service, _dir, _source) = service_with_source();
        let req = ImportPreviewRequest {
            sheet: None,
            source: "../outside.csv".into(),
            group_column: None,
            group_name: None,
        };
        let err = service.preview(&req).expect_err("escape rejected");
        assert!(matches!(err, ImportError::Parse(msg) if msg.contains("escapes")));
    }

    #[test]
    fn commit_publishes_one_valid_file_per_group() {
        let (service, dir, source) = service_with_source();
        let req = ImportCommitRequest {
            sheet: None,
            source,
            group_column: "Site".into(),
            group_name: None,
            visible_id_column: None,
            elemental_columns: None,
            recipe: Some(ImportRecipeDto {
                zero_as_na: false,
                negative_as_na: false,
                na_as_zero: false,
                blank_non_element_label: Some("[blank]".into()),
            }),
            destination_dir: None,
        };
        let resp = service.commit(&req).expect("commit");
        assert_eq!(resp.groups.len(), 2);
        assert_eq!(resp.groups[0].group_name, "Baca");
        assert_eq!(resp.groups[0].path, "groups/Baca.parquet");
        assert_eq!(resp.groups[0].row_count, 2);
        assert_eq!(resp.groups[1].group_name, "Hooper");
        assert_eq!(resp.groups[1].path, "groups/Hooper.parquet");

        let root = dir.path();
        for group in &resp.groups {
            let data = read_group_file(&root.join(&group.path)).expect("group validates");
            assert_eq!(data.profile.group_id, group.group_id);
            assert_eq!(data.profile.revision_id, "rev-1");
            assert_eq!(data.profile.source_path.as_deref(), Some("mini.csv"));
            assert!(!data.profile.measured_elemental_checksum.is_empty());
        }
        // The blank Sub_region cell in Hooper received the recipe label;
        // descriptive roles exclude rowid/anid/elemental, so index 1.
        let hooper = read_group_file(&root.join("groups/Hooper.parquet")).expect("hooper");
        assert_eq!(hooper.rows[0].descriptive[0].as_deref(), Some("Hooper"));
        assert_eq!(hooper.rows[0].descriptive[1].as_deref(), Some("[blank]"));

        // Scan discovers both as ready-to-add, sorted by path. `scan_project`
        // returns absolute canonical paths under the project root; canonicalize
        // the expectation because macOS /tmp is a symlink (/var -> /private/var).
        let root = std::fs::canonicalize(dir.path()).expect("canonical project root");
        let scanned = service.scan().expect("scan");
        assert_eq!(scanned.len(), 2);
        assert_eq!(scanned[0].path, root.join("groups/Baca.parquet"));
        assert_eq!(scanned[1].path, root.join("groups/Hooper.parquet"));
        for candidate in &scanned {
            assert!(matches!(candidate.status, CandidateStatus::ReadyToAdd(_)));
        }
    }

    #[test]
    fn xlsx_and_tsv_sources_preview_and_commit() {
        let dir = tempdir::tempdir().expect("tempdir");
        let mut book = rust_xlsxwriter::Workbook::new();
        let notes = book.add_worksheet();
        notes.set_name("Notes").expect("name");
        notes.write_string(0, 0, "read me").expect("w");
        let data = book.add_worksheet();
        data.set_name("INAA").expect("name");
        for (col, name) in ["anid", "Site", "fe", "na"].iter().enumerate() {
            data.write_string(0, col as u16, *name).expect("w");
        }
        for (row, (anid, site, fe, na)) in [("A1", "Baca", 2.5, 1.0), ("A2", "Zuni", 3.0, 2.0)]
            .iter()
            .enumerate()
        {
            let r = row as u32 + 1;
            data.write_string(r, 0, *anid).expect("w");
            data.write_string(r, 1, *site).expect("w");
            data.write_number(r, 2, *fe).expect("w");
            data.write_number(r, 3, *na).expect("w");
        }
        book.save(dir.path().join("book.xlsx")).expect("save");
        let service = ImportService::new(dir.path()).expect("service");

        // Default sheet is the first non-empty one ("Notes").
        let preview = service
            .preview(&ImportPreviewRequest {
                sheet: None,
                source: "book.xlsx".into(),
                group_column: None,
                group_name: None,
            })
            .expect("preview default sheet");
        assert_eq!(preview.sheet.as_deref(), Some("Notes"));
        assert_eq!(preview.sheets, vec!["Notes", "INAA"]);

        let preview = service
            .preview(&ImportPreviewRequest {
                sheet: Some("INAA".into()),
                source: "book.xlsx".into(),
                group_column: Some("Site".into()),
                group_name: None,
            })
            .expect("preview INAA");
        assert_eq!(preview.row_count, 2);
        assert_eq!(preview.id_column.as_deref(), Some("anid"));
        assert_eq!(preview.elemental_columns, vec!["fe", "na"]);
        assert_eq!(preview.partitions.len(), 2);

        let committed = service
            .commit(&ImportCommitRequest {
                sheet: Some("INAA".into()),
                source: "book.xlsx".into(),
                group_column: "Site".into(),
                group_name: None,
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
            })
            .expect("commit xlsx");
        assert_eq!(committed.groups.len(), 2);
        let baca = read_group_file(&dir.path().join(&committed.groups[0].path)).expect("baca");
        assert_eq!(baca.rows.len(), 1);

        // TSV columns split on tabs, so the same partitioning works.
        std::fs::write(
            dir.path().join("tab.tsv"),
            "anid\tSite\tfe\nT1\tBaca\t1.5\nT2\tHooper\t2.5\n",
        )
        .expect("write tsv");
        let preview = service
            .preview(&ImportPreviewRequest {
                sheet: None,
                source: "tab.tsv".into(),
                group_column: Some("Site".into()),
                group_name: None,
            })
            .expect("preview tsv");
        assert_eq!(preview.columns, vec!["rowid", "anid", "Site", "fe"]);
        assert_eq!(preview.partitions.len(), 2);
        assert!(preview.sheets.is_empty());
    }

    #[test]
    fn commit_waits_for_exclusive_store_project_lock() {
        let (service, dir, source) = service_with_source();
        let locker = FsGroupFileStore::new(dir.path()).expect("store lock handle");
        let lock = locker.lock_exclusive().expect("exclusive project lock");
        let request = ImportCommitRequest {
            sheet: None,
            source,
            group_column: "Site".into(),
            group_name: None,
            visible_id_column: None,
            elemental_columns: None,
            recipe: None,
            destination_dir: None,
        };
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            done_tx
                .send(service.commit(&request))
                .expect("send commit result");
        });

        assert!(done_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err());
        assert!(
            !dir.path().join("groups").exists(),
            "commit does not publish before acquiring the lock"
        );
        drop(lock);
        let response = done_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("commit completes after lock release")
            .expect("commit succeeds");
        assert_eq!(response.groups.len(), 2);
        worker.join().expect("commit worker joins");
    }

    #[test]
    fn preview_and_commit_can_assign_every_row_to_one_named_group() {
        let (service, dir, source) = service_with_source();
        let preview = service
            .preview(&ImportPreviewRequest {
                sheet: None,
                source: source.clone(),
                group_column: None,
                group_name: Some("Reference Set".into()),
            })
            .expect("single-group preview");
        assert_eq!(preview.partitions.len(), 1);
        assert_eq!(preview.partitions[0].group_name, "Reference Set");
        assert_eq!(preview.partitions[0].row_count, 3);
        assert_eq!(
            preview.partitions[0].suggested_path,
            "groups/Reference_Set.parquet"
        );

        let committed = service
            .commit(&ImportCommitRequest {
                sheet: None,
                source,
                group_column: String::new(),
                group_name: Some("Reference Set".into()),
                visible_id_column: Some("anid".into()),
                elemental_columns: Some(vec!["as".into(), "fe".into()]),
                recipe: None,
                destination_dir: None,
            })
            .expect("single-group commit");
        assert_eq!(committed.groups.len(), 1);
        assert_eq!(committed.groups[0].group_name, "Reference Set");
        assert_eq!(committed.groups[0].path, "groups/Reference_Set.parquet");
        assert_eq!(committed.groups[0].row_count, 3);
        let data = read_group_file(&dir.path().join(&committed.groups[0].path))
            .expect("committed group validates");
        assert_eq!(data.profile.group_id, "Reference_Set");
        assert_eq!(data.rows.len(), 3);
    }

    #[test]
    fn commit_requires_exactly_one_nonblank_grouping_mode() {
        let (service, _dir, source) = service_with_source();
        for (group_column, group_name) in [
            ("".to_string(), None),
            ("Site".to_string(), Some("  ".to_string())),
            ("Site".to_string(), Some("Named".to_string())),
        ] {
            let error = service
                .commit(&ImportCommitRequest {
                    sheet: None,
                    source: source.clone(),
                    group_column,
                    group_name,
                    visible_id_column: None,
                    elemental_columns: None,
                    recipe: None,
                    destination_dir: None,
                })
                .expect_err("invalid grouping mode rejected");
            assert!(matches!(error, ImportError::Parse(_)));
        }
    }

    #[test]
    fn commit_rejects_blank_group_values() {
        let dir = tempdir::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("blank.csv"),
            "anid,Site,as\nA1,Baca,1\nA2,,2\n",
        )
        .expect("write");
        let service = ImportService::new(dir.path()).expect("service");
        let err = service
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "blank.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect_err("blank group rejected");
        assert!(matches!(err, ImportError::Parse(msg) if msg.contains("blank")));
        // Nothing was published for the rejected import.
        assert!(!dir.path().join("groups").exists());
    }

    #[test]
    fn default_measured_columns_exclude_text_columns() {
        let dir = tempdir::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("plain.csv"),
            "anid,Site,Depth,Weight
A1,Baca,1.5,10
A2,Zuni,2,11
",
        )
        .expect("write");
        let service = ImportService::new(dir.path()).expect("service");
        let preview = service
            .preview(&ImportPreviewRequest {
                sheet: None,
                source: "plain.csv".into(),
                group_column: None,
                group_name: None,
            })
            .expect("preview");
        assert_eq!(preview.elemental_columns, vec!["Depth", "Weight"]);
        let committed = service
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "plain.csv".into(),
                group_column: "Site".into(),
                group_name: None,
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
            })
            .expect("commit");
        let data = read_group_file(&dir.path().join(&committed.groups[0].path)).expect("read");
        assert_eq!(data.profile.roles.elemental, vec!["Depth", "Weight"]);
        assert_eq!(data.profile.roles.descriptive, vec!["Site"]);
    }

    #[test]
    fn automatic_ids_use_row_numbers_even_when_anid_exists() {
        let (service, dir, source) = service_with_source();
        let base = ImportCommitRequest {
            sheet: None,
            source,
            group_column: String::new(),
            group_name: Some("All".into()),
            visible_id_column: Some(String::new()),
            elemental_columns: Some(vec!["as".into(), "fe".into()]),
            recipe: None,
            destination_dir: Some("auto".into()),
        };
        let committed = service.commit(&base).expect("automatic ids");
        let data = read_group_file(&dir.path().join(&committed.groups[0].path)).expect("read");
        assert_eq!(data.profile.roles.visible_id, "rowid");
        assert_eq!(data.rows[0].visible.as_deref(), Some("1"));
        // The anid column is kept as descriptive data instead.
        assert_eq!(data.profile.roles.descriptive[0], "anid");

        // Omitting the choice keeps the legacy anid default.
        let legacy = service
            .commit(&ImportCommitRequest {
                visible_id_column: None,
                destination_dir: Some("legacy".into()),
                ..base
            })
            .expect("legacy default");
        let data = read_group_file(&dir.path().join(&legacy.groups[0].path)).expect("read");
        assert_eq!(data.profile.roles.visible_id, "anid");
        assert_eq!(data.rows[0].visible.as_deref(), Some("A1"));
    }

    #[test]
    fn commit_dedupes_sanitized_group_names() {
        let dir = tempdir::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("collide.csv"),
            "anid,Site,as\nA1,A/B,1\nA2,A_B,2\n",
        )
        .expect("write");
        let service = ImportService::new(dir.path()).expect("service");
        let resp = service
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "collide.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: Some(vec!["as".into()]),
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        assert_eq!(resp.groups[0].path, "groups/A_B.parquet");
        assert_eq!(resp.groups[1].path, "groups/A_B_2.parquet");
    }

    fn site_request(source: &str, folder: &str) -> ImportCommitRequest {
        ImportCommitRequest {
            sheet: None,
            source: source.into(),
            group_column: "Site".into(),
            group_name: None,
            visible_id_column: Some("anid".into()),
            elemental_columns: Some(vec!["as".into(), "fe".into()]),
            recipe: None,
            destination_dir: Some(folder.into()),
        }
    }

    fn put(dir: &tempdir::TempDir, name: &str, body: &str) {
        std::fs::write(dir.path().join(name), body).expect("write source");
    }

    fn with_mode(mode: ImportMode) -> ImportCommitOptions {
        ImportCommitOptions {
            descriptive_columns: None,
            mode: Some(mode),
        }
    }

    #[test]
    fn descriptive_column_choice_drops_unchecked_columns() {
        let (service, dir, source) = service_with_source();
        let committed = service
            .commit_with(
                &site_request(&source, "Site"),
                &ImportCommitOptions {
                    descriptive_columns: Some(vec!["Site".into()]),
                    mode: Some(ImportMode::NewGroups),
                },
            )
            .expect("commit with column choice");
        let data = read_group_file(&dir.path().join(&committed.groups[0].path)).expect("read");
        assert_eq!(data.profile.roles.descriptive, vec!["Site"]);
        assert_eq!(data.rows[0].descriptive.len(), 1);

        let unknown = service
            .commit_with(
                &site_request(&source, "Other"),
                &ImportCommitOptions {
                    descriptive_columns: Some(vec!["Nope".into()]),
                    mode: Some(ImportMode::NewGroups),
                },
            )
            .expect_err("unknown descriptive column refused");
        assert!(unknown.to_string().contains("Nope"));
        assert!(!dir.path().join("Other").exists());
    }

    #[test]
    fn preview_reports_existing_groups_and_same_source() {
        let (service, _dir, source) = service_with_source();
        service
            .commit(&site_request(&source, "Site"))
            .expect("first import");
        let preview = service
            .preview(&ImportPreviewRequest {
                sheet: None,
                source: source.clone(),
                group_column: Some("Site".into()),
                group_name: None,
            })
            .expect("preview");
        assert!(preview.source_sha256.is_some());
        let paths: Vec<&str> = preview
            .existing_groups
            .iter()
            .map(|g| g.path.as_str())
            .collect();
        assert_eq!(paths, vec!["Site/Baca.parquet", "Site/Hooper.parquet"]);
        assert!(preview.existing_groups.iter().all(|g| g.same_source));
        assert!(preview.existing_groups.iter().all(|g| g.folder == "Site"));
    }

    #[test]
    fn new_groups_mode_refuses_reimport_and_name_clashes() {
        let (service, dir, source) = service_with_source();
        let options = with_mode(ImportMode::NewGroups);
        service
            .commit_with(&site_request(&source, "Site"), &options)
            .expect("first import");
        let again = service
            .commit_with(&site_request(&source, "Site"), &options)
            .expect_err("same source refused");
        assert!(again.to_string().contains("imported from this file"));
        // Another file with the same group names clashes by file name.
        put(&dir, "other.csv", "anid,Site,as,fe\nB1,Baca,1,2\n");
        let clash = service
            .commit_with(&site_request("other.csv", "Site"), &options)
            .expect_err("name clash refused");
        assert!(clash.to_string().contains("Baca.parquet"));
        // Nothing was suffixed or duplicated.
        assert!(!dir.path().join("Site/Baca_2.parquet").exists());
        // A new folder works.
        service
            .commit_with(&site_request("other.csv", "Site 2"), &options)
            .expect("new folder");
    }

    #[test]
    fn replace_source_is_journaled_and_keeps_sample_identities() {
        let (service, dir, source) = service_with_source();
        let first = service
            .commit(&site_request(&source, "Site"))
            .expect("first import");
        let before = read_group_file(&dir.path().join(&first.groups[0].path)).expect("read");

        // Replace with a different measured choice.
        let mut request = site_request(&source, "Site");
        request.elemental_columns = Some(vec!["fe".into()]);
        let replaced = service
            .commit_with(&request, &with_mode(ImportMode::ReplaceSource))
            .expect("replace");
        assert_eq!(replaced.groups.len(), 2);
        assert_eq!(replaced.groups[0].revision_id, "rev-2");
        let after = read_group_file(&dir.path().join(&replaced.groups[0].path)).expect("read");
        assert_eq!(after.profile.roles.elemental, vec!["fe"]);
        let ids = |data: &GroupFileData| data.rows.iter().map(|r| r.uuid).collect::<Vec<_>>();
        assert_eq!(
            ids(&after),
            ids(&before),
            "identities preserved by row number"
        );
        // Journaled: the transaction directory is cleaned up after commit and
        // the replaced original is archived in the history.
        let transactions = dir.path().join(".archaeodash/transactions");
        let leftover = std::fs::read_dir(&transactions)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(leftover, 0);

        // Regrouped into one named group: the old files that are no longer
        // written are deleted in the same transaction.
        let single = ImportCommitRequest {
            group_column: String::new(),
            group_name: Some("Everything".into()),
            ..site_request(&source, "Site")
        };
        let response = service
            .commit_with(&single, &with_mode(ImportMode::ReplaceSource))
            .expect("replace with one group");
        assert_eq!(response.groups.len(), 1);
        assert!(!dir.path().join("Site/Baca.parquet").exists());
        assert!(!dir.path().join("Site/Hooper.parquet").exists());
        let everything =
            read_group_file(&dir.path().join("Site/Everything.parquet")).expect("read");
        assert_eq!(everything.rows.len(), 3);
    }

    #[test]
    fn replace_source_refuses_folders_without_that_source() {
        let (service, dir, source) = service_with_source();
        service
            .commit(&site_request(&source, "Site"))
            .expect("first import");
        put(&dir, "other.csv", "anid,Site,as,fe\nB1,Zuni,1,2\n");
        let err = service
            .commit_with(
                &site_request("other.csv", "Site"),
                &with_mode(ImportMode::ReplaceSource),
            )
            .expect_err("nothing to replace");
        let message = err.to_string();
        assert!(message.contains("no groups imported from this file"));
        // A file of the same name from another source is never replaced.
        service
            .commit(&site_request("other.csv", "Site"))
            .expect("other import");
        let single = ImportCommitRequest {
            group_column: String::new(),
            group_name: Some("Zuni".into()),
            ..site_request(&source, "Site")
        };
        let err = service
            .commit_with(&single, &with_mode(ImportMode::ReplaceSource))
            .expect_err("other source not replaced");
        assert!(err.to_string().contains("was not"));
        let zuni = read_group_file(&dir.path().join("Site/Zuni.parquet")).expect("read");
        assert_eq!(zuni.rows[0].visible.as_deref(), Some("B1"));
        assert!(dir.path().join("Site/Baca.parquet").exists());
    }

    #[test]
    fn append_adds_rows_by_group_name_and_aligns_descriptive_columns() {
        let (service, dir, source) = service_with_source();
        service
            .commit(&site_request(&source, "Site"))
            .expect("first import");
        let baca_before = read_group_file(&dir.path().join("Site/Baca.parquet")).expect("read");
        // New rows: one more Baca sample, a new Zuni group, a new descriptive
        // column "Ware", and no Sub-region column.
        put(
            &dir,
            "more.csv",
            "anid,Site,Ware,as,fe\nM1,Baca,Red,1,2\nM2,Zuni,Buff,3,\n",
        );
        let mut request = site_request("more.csv", "Site");
        request.elemental_columns = Some(vec!["as".into(), "fe".into()]);
        let response = service
            .commit_with(&request, &with_mode(ImportMode::Append))
            .expect("append");
        let names: Vec<&str> = response
            .groups
            .iter()
            .map(|g| g.group_name.as_str())
            .collect();
        assert_eq!(names, vec!["Baca", "Zuni"]);

        let baca = read_group_file(&dir.path().join("Site/Baca.parquet")).expect("read");
        assert_eq!(baca.rows.len(), 3);
        assert_eq!(baca.profile.revision_id, "rev-2");
        assert_eq!(
            baca.profile.roles.descriptive,
            vec!["Site", "Sub_region", "Ware"]
        );
        // Old rows keep identities and measured values; new rows are new.
        assert_eq!(baca.rows[0].uuid, baca_before.rows[0].uuid);
        assert_eq!(baca.rows[0].elemental, baca_before.rows[0].elemental);
        assert_eq!(baca.rows[0].descriptive[2], None);
        assert_eq!(baca.rows[2].visible.as_deref(), Some("M1"));
        assert_eq!(baca.rows[2].descriptive[1], None);
        assert_eq!(baca.rows[2].descriptive[2].as_deref(), Some("Red"));
        // Siblings that got no rows are realigned to the same columns.
        let hooper = read_group_file(&dir.path().join("Site/Hooper.parquet")).expect("read");
        assert_eq!(
            hooper.profile.roles.descriptive,
            baca.profile.roles.descriptive
        );
        let zuni = read_group_file(&dir.path().join("Site/Zuni.parquet")).expect("read");
        assert_eq!(
            zuni.profile.roles.descriptive,
            baca.profile.roles.descriptive
        );

        // "fe" left unchecked but numeric: taken as measured for Hooper, like
        // the legacy type harmonization.
        put(&dir, "fe.csv", "anid,Site,as,fe\nF1,Hooper,1,2.5\n");
        let mut request = site_request("fe.csv", "Site");
        request.elemental_columns = Some(vec!["as".into()]);
        service
            .commit_with(&request, &with_mode(ImportMode::Append))
            .expect("harmonized append");
        let hooper = read_group_file(&dir.path().join("Site/Hooper.parquet")).expect("read");
        assert_eq!(
            hooper.rows.last().expect("row").elemental,
            vec![Some(1.0), Some(2.5)]
        );
    }

    #[test]
    fn append_refuses_measured_mismatches_and_repeated_sources() {
        let (service, dir, source) = service_with_source();
        service
            .commit(&site_request(&source, "Site"))
            .expect("first import");
        let again = service
            .commit_with(
                &site_request(&source, "Site"),
                &with_mode(ImportMode::Append),
            )
            .expect_err("same file appended twice");
        assert!(again.to_string().contains("already in folder"));

        put(&dir, "extra.csv", "anid,Site,as,fe,na\nX1,Baca,1,2,3\n");
        let mut request = site_request("extra.csv", "Site");
        request.elemental_columns = Some(vec!["as".into(), "fe".into(), "na".into()]);
        let err = service
            .commit_with(&request, &with_mode(ImportMode::Append))
            .expect_err("extra measured column refused");
        assert!(err.to_string().contains("does not measure na"));

        put(&dir, "text.csv", "anid,Site,as,fe\nT1,Baca,1,high\n");
        let mut request = site_request("text.csv", "Site");
        request.elemental_columns = Some(vec!["as".into()]);
        let err = service
            .commit_with(&request, &with_mode(ImportMode::Append))
            .expect_err("non-numeric measured column refused");
        assert!(err.to_string().contains("not all numbers"));
        // Refusals leave the group untouched.
        let baca = read_group_file(&dir.path().join("Site/Baca.parquet")).expect("read");
        assert_eq!(baca.profile.revision_id, "rev-1");
        assert_eq!(baca.rows.len(), 2);
    }

    #[test]
    fn worksheets_of_one_workbook_are_different_sources() {
        let dir = tempdir::tempdir().expect("tempdir");
        let mut book = rust_xlsxwriter::Workbook::new();
        for (sheet, site) in [("A", "Baca"), ("B", "Zuni")] {
            let data = book.add_worksheet();
            data.set_name(sheet).expect("name");
            for (col, name) in ["anid", "Site", "as", "fe"].iter().enumerate() {
                data.write_string(0, col as u16, *name).expect("w");
            }
            data.write_string(1, 0, format!("{sheet}1")).expect("w");
            data.write_string(1, 1, site).expect("w");
            data.write_number(1, 2, 1.0).expect("w");
            data.write_number(1, 3, 2.0).expect("w");
        }
        book.save(dir.path().join("book.xlsx")).expect("save");
        let service = ImportService::new(dir.path()).expect("service");
        let request = |sheet: &str| ImportCommitRequest {
            sheet: Some(sheet.into()),
            ..site_request("book.xlsx", "Site")
        };
        let first = service
            .commit_with(&request("A"), &with_mode(ImportMode::NewGroups))
            .expect("sheet A");
        let data = read_group_file(&dir.path().join(&first.groups[0].path)).expect("read");
        assert_eq!(data.profile.source_path.as_deref(), Some("book.xlsx#A"));
        let preview = service
            .preview(&ImportPreviewRequest {
                sheet: Some("B".into()),
                source: "book.xlsx".into(),
                group_column: Some("Site".into()),
                group_name: None,
            })
            .expect("preview B");
        assert!(preview.existing_groups.iter().all(|g| !g.same_source));
        service
            .commit_with(&request("B"), &with_mode(ImportMode::NewGroups))
            .expect("sheet B is another source");
        let again = service
            .commit_with(&request("A"), &with_mode(ImportMode::NewGroups))
            .expect_err("sheet A again");
        assert!(again.to_string().contains("imported from this file"));
    }

    #[test]
    fn worksheet_names_with_hash_are_told_apart() {
        let dir = tempdir::tempdir().expect("tempdir");
        let mut book = rust_xlsxwriter::Workbook::new();
        for (sheet, site) in [("Data#1", "Baca"), ("Data#2", "Zuni")] {
            let data = book.add_worksheet();
            data.set_name(sheet).expect("name");
            for (col, name) in ["anid", "Site", "as", "fe"].iter().enumerate() {
                data.write_string(0, col as u16, *name).expect("w");
            }
            data.write_string(1, 0, format!("{sheet}-1")).expect("w");
            data.write_string(1, 1, site).expect("w");
            data.write_number(1, 2, 1.0).expect("w");
            data.write_number(1, 3, 2.0).expect("w");
        }
        book.save(dir.path().join("book.xlsx")).expect("save");
        let service = ImportService::new(dir.path()).expect("service");
        let request = |sheet: &str| ImportCommitRequest {
            sheet: Some(sheet.into()),
            ..site_request("book.xlsx", "Site")
        };
        service
            .commit_with(&request("Data#1"), &with_mode(ImportMode::NewGroups))
            .expect("first sheet");
        // The other worksheet is another source: nothing of it to replace.
        let err = service
            .commit_with(&request("Data#2"), &with_mode(ImportMode::ReplaceSource))
            .expect_err("another worksheet is not this source");
        assert!(err
            .to_string()
            .contains("no groups imported from this file"));
        let baca = read_group_file(&dir.path().join("Site/Baca.parquet")).expect("read");
        assert_eq!(baca.rows[0].visible.as_deref(), Some("Data#1-1"));
        assert!(!dir.path().join("Site/Zuni.parquet").exists());
    }

    #[test]
    fn replace_refuses_groups_holding_rows_from_other_files() {
        let (service, dir, source) = service_with_source();
        service
            .commit(&site_request(&source, "Site"))
            .expect("first import");
        put(&dir, "more.csv", "anid,Site,as,fe\nM1,Baca,7,8\n");
        service
            .commit_with(
                &site_request("more.csv", "Site"),
                &with_mode(ImportMode::Append),
            )
            .expect("append");
        let err = service
            .commit_with(
                &site_request(&source, "Site"),
                &with_mode(ImportMode::ReplaceSource),
            )
            .expect_err("the appended row would be dropped");
        assert!(err.to_string().contains("not imported from this file"));
        let baca = read_group_file(&dir.path().join("Site/Baca.parquet")).expect("read");
        assert_eq!(baca.rows.len(), 3);
        assert_eq!(baca.profile.revision_id, "rev-2");

        // Automatic IDs: the appended row has the same row number and ID as a
        // source row, so only its values tell it apart.
        let automatic = |source: &str| ImportCommitRequest {
            visible_id_column: Some(AUTOMATIC_ID_COLUMN.into()),
            ..site_request(source, "Auto")
        };
        service
            .commit_with(&automatic(&source), &with_mode(ImportMode::NewGroups))
            .expect("automatic import");
        service
            .commit_with(&automatic("more.csv"), &with_mode(ImportMode::Append))
            .expect("automatic append");
        let err = service
            .commit_with(&automatic(&source), &with_mode(ImportMode::ReplaceSource))
            .expect_err("the appended row would be dropped");
        assert!(err.to_string().contains("not imported from this file"));
        let baca = read_group_file(&dir.path().join("Auto/Baca.parquet")).expect("read");
        assert_eq!(baca.rows.len(), 3);
    }

    #[test]
    fn append_refuses_other_id_columns_and_role_clashes() {
        let (service, dir, source) = service_with_source();
        service
            .commit(&site_request(&source, "Site"))
            .expect("first import");
        put(&dir, "more.csv", "anid,Site,as,fe\nM1,Baca,1,2\n");
        let mut request = site_request("more.csv", "Site");
        request.visible_id_column = Some(AUTOMATIC_ID_COLUMN.into());
        let err = service
            .commit_with(&request, &with_mode(ImportMode::Append))
            .expect_err("another ID column refused");
        assert!(err.to_string().contains("as its ID column"));

        // A new group whose unchecked numeric column the folder measures:
        // the column cannot be descriptive in one group and measured in
        // another of the same folder.
        put(&dir, "zuni.csv", "anid,Site,as,fe\nZ1,Zuni,1,2\n");
        let mut request = site_request("zuni.csv", "Site");
        request.elemental_columns = Some(vec!["as".into()]);
        let err = service
            .commit_with(&request, &with_mode(ImportMode::Append))
            .expect_err("role clash refused");
        assert!(err.to_string().contains("fe is descriptive"));

        let baca = read_group_file(&dir.path().join("Site/Baca.parquet")).expect("read");
        assert_eq!(baca.profile.revision_id, "rev-1");
        assert_eq!(baca.rows.len(), 2);
        assert!(!dir.path().join("Site/Zuni.parquet").exists());
    }

    #[test]
    fn import_finds_an_existing_folder_spelled_in_another_case() {
        let (service, dir, source) = service_with_source();
        service
            .commit(&site_request(&source, "Site"))
            .expect("first import");
        let single = ImportCommitRequest {
            group_column: String::new(),
            group_name: Some("Everything".into()),
            ..site_request(&source, "SITE")
        };
        let result = service.commit_with(&single, &with_mode(ImportMode::NewGroups));
        if dir.path().join("SITE").join("Baca.parquet").exists() {
            // Case-insensitive file system: "SITE" is the "Site" folder, which
            // already holds this file's samples.
            let err = result.expect_err("same samples twice in one folder");
            assert!(err.to_string().contains("imported from this file"));
            assert!(!dir.path().join("Site/Everything.parquet").exists());
        } else {
            let response = result.expect("another folder");
            assert_eq!(response.groups[0].path, "SITE/Everything.parquet");
        }
    }
}
