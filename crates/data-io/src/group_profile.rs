//! ArchaeoDash Group Parquet profile v1 (Sections 6.6, 7.2).
//!
//! One self-describing Parquet file per logical group. Measured elemental
//! values are immutable Float64 columns; the hidden `analytical_uuid` is a
//! 16-byte fixed binary column; profile metadata lives under the
//! `archaeodash.profile.v1` key as canonical JSON (sorted keys, UTF-8).
//!
//! `measured_elemental_checksum` is SHA-256 over the canonical ordered
//! `(analytical_uuid, column-id, value|null)` tuple stream in schema order —
//! never over raw file bytes (Section 17.1 item 8).

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use arrow::array::{Array, FixedSizeBinaryArray, Float64Array, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use parquet::file::reader::{FileReader, SerializedFileReader};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::rnum::parse_r_numeric;
use crate::{ImportError, TextFrame};

pub const PROFILE_KEY: &str = "archaeodash.profile.v1";
pub const PROFILE_VERSION: i64 = 1;
pub const IDENTITY_COLUMN: &str = "analytical_uuid";
pub const LEGACY_ROWID_COLUMN: &str = "legacy_rowid";
const UUID_BYTE_LEN: usize = 16;

/// Import value-class policies (Section 7.2 item 11). Applied in order at
/// import; the recipe is recorded in the group profile.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportRecipe {
    /// Treat zero elemental values as NA.
    pub zero_as_na: bool,
    /// Treat negative elemental values as NA.
    pub negative_as_na: bool,
    /// Replace NA elemental values with zero (applied last).
    pub na_as_zero: bool,
    /// Optional `[blank]`-style label for empty non-elemental cells.
    pub blank_non_element_label: Option<String>,
}

/// Column roles stored in the profile (Section 6.6 role metadata).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ColumnRoles {
    pub identity: String,
    pub visible_id: String,
    pub legacy_rowid: String,
    pub descriptive: Vec<String>,
    pub elemental: Vec<String>,
}

/// Profile metadata for one group file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GroupProfile {
    pub profile_version: i64,
    pub file_kind: String,
    pub group_id: String,
    pub group_name: String,
    pub revision_id: String,
    pub roles: ColumnRoles,
    pub row_count: usize,
    pub source_path: Option<String>,
    pub source_sha256: Option<String>,
    pub import_recipe: ImportRecipe,
    pub measured_elemental_checksum: String,
}

/// One partition produced by import: rows sharing a group value.
#[derive(Debug, Clone, PartialEq)]
pub struct Partition {
    pub group_name: String,
    pub row_indices: Vec<usize>,
}

/// Partitions rows by the group column; blank values are rejected (Section
/// 7.2 item 12: explicit destination required).
pub fn partition_by_group(
    frame: &TextFrame,
    group_col: &str,
) -> Result<Vec<Partition>, ImportError> {
    let idx = frame
        .column_index(group_col)
        .ok_or_else(|| ImportError::Parse(format!("missing group column {group_col}")))?;
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, row) in frame.rows.iter().enumerate() {
        let value = match &row[idx] {
            Some(v) if !v.trim().is_empty() => v.clone(),
            _ => {
                return Err(ImportError::Parse(format!(
                    "group column {group_col} has a blank value at source row {}; choose an explicit destination",
                    i + 1
                )))
            }
        };
        if !map.contains_key(&value) {
            order.push(value.clone());
        }
        map.entry(value).or_default().push(i);
    }
    Ok(order
        .into_iter()
        .map(|group_name| {
            let row_indices = map.remove(&group_name).unwrap_or_default();
            Partition {
                group_name,
                row_indices,
            }
        })
        .collect())
}

/// Deterministic, filename-safe group names for partitioned files
/// (Section 7.2 item 12: "deterministic, filename-safe names").
pub fn sanitize_group_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        "group".to_string()
    } else {
        safe
    }
}

/// Canonical measured-elemental checksum: SHA-256 over
/// `uuid\ncol\nvalue\n` tuples in row order, elemental schema order.
pub fn measured_elemental_checksum(
    uuids: &[Uuid],
    elemental_columns: &[String],
    elemental_values: &[Vec<Option<f64>>],
) -> String {
    let mut hasher = Sha256::new();
    for (row, uuid) in uuids.iter().enumerate() {
        for (col, name) in elemental_columns.iter().enumerate() {
            hasher.update(uuid.as_bytes());
            hasher.update(b"\n");
            hasher.update(name.as_bytes());
            hasher.update(b"\n");
            match elemental_values[col][row] {
                Some(v) if v.is_finite() => {
                    hasher.update(crate::rnum::r_format_double(v).as_bytes())
                }
                _ => hasher.update(b"null"),
            }
            hasher.update(b"\n");
        }
    }
    format!("{:x}", hasher.finalize())
}

fn apply_recipe(raw: Option<f64>, recipe: &ImportRecipe) -> Option<f64> {
    let mut value = raw;
    if let Some(v) = value {
        if recipe.zero_as_na && v == 0.0 {
            value = None;
        }
        if recipe.negative_as_na && v.is_finite() && v < 0.0 {
            value = None;
        }
    }
    if value.is_none() && recipe.na_as_zero {
        value = Some(0.0);
    }
    value
}

/// One analytical-unit row of a group file, aligned with the profile roles.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupRow {
    pub uuid: Uuid,
    pub visible: Option<String>,
    pub legacy_rowid: Option<String>,
    /// Descriptive cell values in `roles.descriptive` order.
    pub descriptive: Vec<Option<String>>,
    /// Measured elemental values in `roles.elemental` order. Immutable after
    /// import; group operations must preserve them exactly.
    pub elemental: Vec<Option<f64>>,
}

/// Full row data of one group file plus its validated profile.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupFileData {
    pub profile: GroupProfile,
    pub rows: Vec<GroupRow>,
}

/// Builds and writes one group Parquet file from a partition of a loaded
/// frame. Elemental columns coerce through `parse_r_numeric` with the recipe;
/// descriptive columns keep raw text. Returns the profile plus the checksum.
///
/// `existing_uuids` preserves hidden analytical identities across a revision
/// (Section 5.2: `analytical_uuid` is immutable across group operations);
/// `None` mints fresh UUIDv7 values for a first import.
#[allow(clippy::too_many_arguments)]
pub fn write_group_file(
    path: &Path,
    group_id: &str,
    revision_id: &str,
    visible_id_col: &str,
    elemental: &[String],
    recipe: &ImportRecipe,
    frame: &TextFrame,
    partition: &Partition,
    source_path: Option<String>,
    source_sha256: Option<String>,
    existing_uuids: Option<&[Uuid]>,
) -> Result<GroupProfile, ImportError> {
    let n = partition.row_indices.len();
    if n == 0 {
        return Err(ImportError::Parse("cannot write an empty group".into()));
    }
    let mut descriptive: Vec<String> = Vec::new();
    for name in &frame.columns {
        if name == "rowid" || elemental.contains(name) || *name == visible_id_col {
            continue;
        }
        descriptive.push(name.clone());
    }

    let visible_idx = frame.column_index(visible_id_col);
    let rowid_idx = frame.column_index("rowid");
    let elemental_idx: Vec<Option<usize>> = elemental
        .iter()
        .map(|name| frame.column_index(name))
        .collect();
    let descriptive_idx: Vec<Option<usize>> = descriptive
        .iter()
        .map(|name| frame.column_index(name))
        .collect();
    if elemental_idx.iter().any(|c| c.is_none()) {
        return Err(ImportError::Parse(
            "elemental column missing from frame".into(),
        ));
    }

    let mut rows: Vec<GroupRow> = Vec::with_capacity(n);
    for (row, &src) in partition.row_indices.iter().enumerate() {
        let uuid = match existing_uuids {
            Some(existing) => existing[row],
            None => Uuid::now_v7(),
        };
        let visible = visible_idx.and_then(|i| frame.rows[src][i].clone());
        let legacy_rowid = rowid_idx.and_then(|i| frame.rows[src][i].clone());
        let descriptive_values: Vec<Option<String>> = descriptive_idx
            .iter()
            .map(|src_idx| {
                let cell = src_idx.and_then(|i| frame.rows[src][i].clone());
                match (&cell, &recipe.blank_non_element_label) {
                    (Some(v), Some(label)) if v.trim().is_empty() => Some(label.clone()),
                    (None, Some(label)) => Some(label.clone()),
                    (other, _) => other.clone(),
                }
            })
            .collect();
        let elemental_values: Vec<Option<f64>> = elemental_idx
            .iter()
            .map(|src_idx| {
                let raw = src_idx
                    .and_then(|i| frame.rows[src][i].as_deref())
                    .and_then(parse_r_numeric);
                apply_recipe(raw, recipe)
            })
            .collect();
        rows.push(GroupRow {
            uuid,
            visible,
            legacy_rowid,
            descriptive: descriptive_values,
            elemental: elemental_values,
        });
    }

    let profile = GroupProfile {
        profile_version: PROFILE_VERSION,
        file_kind: "group".to_string(),
        group_id: group_id.to_string(),
        group_name: partition.group_name.clone(),
        revision_id: revision_id.to_string(),
        roles: ColumnRoles {
            identity: IDENTITY_COLUMN.to_string(),
            visible_id: visible_id_col.to_string(),
            legacy_rowid: LEGACY_ROWID_COLUMN.to_string(),
            descriptive,
            elemental: elemental.to_vec(),
        },
        row_count: n,
        source_path,
        source_sha256,
        import_recipe: recipe.clone(),
        measured_elemental_checksum: String::new(),
    };
    write_group_rows(path, profile, &rows)
}

/// Writes a successor group file from full row data (Section 6.7: every
/// mutation is a full staged rewrite). `row_count` and
/// `measured_elemental_checksum` in the given profile template are
/// recomputed from the rows; all other profile fields pass through. The write
/// is atomic: temp file, fsync, rename, parent-dir sync.
pub fn write_group_rows(
    path: &Path,
    mut profile: GroupProfile,
    rows: &[GroupRow],
) -> Result<GroupProfile, ImportError> {
    if rows.is_empty() {
        return Err(ImportError::Parse("cannot write an empty group".into()));
    }
    let roles = &profile.roles;
    for row in rows {
        if row.descriptive.len() != roles.descriptive.len()
            || row.elemental.len() != roles.elemental.len()
        {
            return Err(ImportError::Parse(
                "row arity does not match declared roles".into(),
            ));
        }
    }

    let uuids: Vec<Uuid> = rows.iter().map(|r| r.uuid).collect();
    let elemental_values: Vec<Vec<Option<f64>>> = roles
        .elemental
        .iter()
        .enumerate()
        .map(|(col, _)| rows.iter().map(|r| r.elemental[col]).collect())
        .collect();
    profile.row_count = rows.len();
    profile.measured_elemental_checksum =
        measured_elemental_checksum(&uuids, &roles.elemental, &elemental_values);

    let mut metadata = HashMap::new();
    metadata.insert(
        PROFILE_KEY.to_string(),
        serde_json::to_string(&profile)
            .map_err(|e| ImportError::Parse(format!("profile serialize: {e}")))?,
    );
    let mut fields = vec![
        Field::new(
            IDENTITY_COLUMN,
            DataType::FixedSizeBinary(UUID_BYTE_LEN as i32),
            false,
        ),
        Field::new(roles.visible_id.clone(), DataType::Utf8, true),
        Field::new(roles.legacy_rowid.clone(), DataType::Utf8, true),
    ];
    for name in &roles.descriptive {
        fields.push(Field::new(name.clone(), DataType::Utf8, true));
    }
    for name in &roles.elemental {
        fields.push(Field::new(name.clone(), DataType::Float64, true));
    }
    let schema = Schema::new_with_metadata(fields, metadata);

    let mut uuid_builder =
        arrow::array::FixedSizeBinaryBuilder::with_capacity(rows.len(), UUID_BYTE_LEN as i32);
    for uuid in &uuids {
        uuid_builder
            .append_value(uuid.as_bytes())
            .map_err(|e| ImportError::Parse(format!("uuid: {e}")))?;
    }
    let uuid_array = uuid_builder.finish();
    let mk_string = |values: Vec<Option<String>>| -> StringArray {
        StringArray::from(values.iter().map(|v| v.as_deref()).collect::<Vec<_>>())
    };
    let mut columns: Vec<ArrayRef> = vec![
        std::sync::Arc::new(uuid_array),
        std::sync::Arc::new(mk_string(rows.iter().map(|r| r.visible.clone()).collect())),
        std::sync::Arc::new(mk_string(
            rows.iter().map(|r| r.legacy_rowid.clone()).collect(),
        )),
    ];
    for col in 0..roles.descriptive.len() {
        columns.push(std::sync::Arc::new(mk_string(
            rows.iter().map(|r| r.descriptive[col].clone()).collect(),
        )));
    }
    for values in &elemental_values {
        columns.push(std::sync::Arc::new(Float64Array::from(values.clone())));
    }
    let batch = RecordBatch::try_new(std::sync::Arc::new(schema), columns)
        .map_err(|e| ImportError::Parse(format!("batch: {e}")))?;

    let parent = path.parent().ok_or_else(|| {
        ImportError::Io("group file path must have a parent directory".to_string())
    })?;
    std::fs::create_dir_all(parent).map_err(|e| ImportError::Io(e.to_string()))?;
    // Write the profile both as an explicit Parquet footer KV entry (visible to
    // any Parquet reader) and inside the Arrow schema metadata (Section 17.1
    // item 8: namespaced key-value metadata). Stage-then-rename so readers
    // never observe a partial file (Section 6.3).
    let props = WriterProperties::builder()
        .set_key_value_metadata(Some(vec![parquet::file::metadata::KeyValue::new(
            PROFILE_KEY.to_string(),
            Some(
                serde_json::to_string(&profile)
                    .map_err(|e| ImportError::Parse(format!("profile serialize: {e}")))?,
            ),
        )]))
        .build();
    let tmp = parent.join(format!(".tmp-group-{}", Uuid::now_v7().simple()));
    let write = || -> Result<(), ImportError> {
        let file = File::create(&tmp).map_err(|e| ImportError::Io(e.to_string()))?;
        let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))
            .map_err(|e| ImportError::Parse(format!("parquet writer: {e}")))?;
        writer
            .write(&batch)
            .map_err(|e| ImportError::Parse(format!("parquet write: {e}")))?;
        writer
            .close()
            .map_err(|e| ImportError::Parse(format!("parquet close: {e}")))?;
        if let Ok(f) = File::open(&tmp) {
            let _ = f.sync_all();
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        ImportError::Io(e.to_string())
    })?;
    crate::export::sync_dir(parent);
    Ok(profile)
}

/// Reads and fully validates a group file, returning its profile and all row
/// data (Section 6.6 validation-on-add, reused for move/copy/merge reads).
pub fn read_group_file(path: &Path) -> Result<GroupFileData, ImportError> {
    let profile = validate_group_file(path)?;
    let file = File::open(path).map_err(|e| ImportError::Io(e.to_string()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| ImportError::Parse(format!("parquet open: {e}")))?;
    let batches = builder
        .build()
        .map_err(|e| ImportError::Parse(format!("parquet read: {e}")))?
        .collect::<Result<Vec<RecordBatch>, _>>()
        .map_err(|e| ImportError::Parse(format!("parquet batch: {e}")))?;

    let roles = &profile.roles;
    let mut rows: Vec<GroupRow> = Vec::with_capacity(profile.row_count);
    for batch in &batches {
        let uuids = batch
            .column_by_name(IDENTITY_COLUMN)
            .ok_or_else(|| ImportError::Parse("identity column missing".into()))?
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .ok_or_else(|| ImportError::Parse("identity column wrong type".into()))?;
        let visible = string_column(batch, &roles.visible_id)?;
        let rowids = string_column(batch, &roles.legacy_rowid)?;
        let descriptive: Vec<StringArray> = roles
            .descriptive
            .iter()
            .map(|name| string_column(batch, name))
            .collect::<Result<_, _>>()?;
        let elemental: Vec<Float64Array> = roles
            .elemental
            .iter()
            .map(|name| {
                batch
                    .column_by_name(name)
                    .ok_or_else(|| ImportError::Parse(format!("column {name} missing")))
                    .and_then(|c| {
                        c.as_any()
                            .downcast_ref::<Float64Array>()
                            .cloned()
                            .ok_or_else(|| ImportError::Parse("elemental column wrong type".into()))
                    })
            })
            .collect::<Result<_, _>>()?;
        for i in 0..batch.num_rows() {
            rows.push(GroupRow {
                uuid: Uuid::from_slice(uuids.value(i))
                    .map_err(|e| ImportError::Parse(format!("uuid: {e}")))?,
                visible: string_at(&visible, i),
                legacy_rowid: string_at(&rowids, i),
                descriptive: descriptive.iter().map(|a| string_at(a, i)).collect(),
                elemental: elemental.iter().map(|a| scalar_at(a, i)).collect(),
            });
        }
    }
    Ok(GroupFileData { profile, rows })
}

fn string_column(batch: &RecordBatch, name: &str) -> Result<StringArray, ImportError> {
    batch
        .column_by_name(name)
        .ok_or_else(|| ImportError::Parse(format!("column {name} missing")))?
        .as_any()
        .downcast_ref::<StringArray>()
        .cloned()
        .ok_or_else(|| ImportError::Parse(format!("column {name} wrong type")))
}

fn string_at(array: &StringArray, i: usize) -> Option<String> {
    if array.is_null(i) {
        None
    } else {
        Some(array.value(i).to_string())
    }
}

fn scalar_at(array: &Float64Array, i: usize) -> Option<f64> {
    if array.is_null(i) {
        None
    } else {
        Some(array.value(i))
    }
}

use arrow::array::ArrayRef;

/// Rejects oversized analytical files from footer dimensions before decoding
/// row data. This is a resource preflight, not full profile validation.
/// Callers must still validate the file; external writers must coordinate
/// with the project store to avoid replacing it between preflight and read.
pub fn check_group_read_limits(
    path: &Path,
    max_rows: u64,
    max_cells: u64,
    max_uncompressed_bytes: u64,
) -> Result<(), ImportError> {
    let file = File::open(path).map_err(|e| ImportError::Io(e.to_string()))?;
    if file
        .metadata()
        .map_err(|e| ImportError::Io(e.to_string()))?
        .len()
        > max_uncompressed_bytes
    {
        return Err(ImportError::Limit(
            "analysis file exceeds the byte limit".into(),
        ));
    }
    let reader = SerializedFileReader::new(file)
        .map_err(|e| ImportError::Parse(format!("parquet open: {e}")))?;
    let metadata = reader.metadata();
    let rows = u64::try_from(metadata.file_metadata().num_rows())
        .map_err(|_| ImportError::Parse("negative Parquet row count".into()))?;
    let columns = metadata.file_metadata().schema_descr().num_columns() as u64;
    let bytes = metadata
        .row_groups()
        .iter()
        .try_fold(0u64, |sum, group| {
            u64::try_from(group.total_byte_size())
                .ok()
                .and_then(|size| sum.checked_add(size))
        })
        .ok_or_else(|| ImportError::Limit("invalid or overflowing Parquet decoded size".into()))?;
    if rows > max_rows || rows.saturating_mul(columns) > max_cells || bytes > max_uncompressed_bytes
    {
        return Err(ImportError::Limit(
            "analysis file exceeds row, cell, or decoded-byte limits".into(),
        ));
    }
    Ok(())
}

/// Reads raw `archaeodash.profile.v1` metadata from a Parquet footer without
/// decoding row data (metadata-only readiness check). Checks the file-level
/// KV entries first, then the embedded Arrow schema metadata that arrow-rs
/// writes under the `ARROW:schema` key.
pub fn read_profile_metadata(path: &Path) -> Result<Option<String>, ImportError> {
    let file = File::open(path).map_err(|e| ImportError::Io(e.to_string()))?;
    let reader = SerializedFileReader::new(file)
        .map_err(|e| ImportError::Parse(format!("parquet open: {e}")))?;
    let kvs = reader
        .metadata()
        .file_metadata()
        .key_value_metadata()
        .cloned()
        .unwrap_or_default();
    if let Some(found) = kvs
        .into_iter()
        .find(|kv| kv.key == PROFILE_KEY)
        .and_then(|kv| kv.value)
    {
        return Ok(Some(found));
    }
    // arrow-rs embeds schema-level metadata inside the base64 `ARROW:schema`
    // footer entry; decode it and look for the profile key there.
    let file2 = File::open(path).map_err(|e| ImportError::Io(e.to_string()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file2)
        .map_err(|e| ImportError::Parse(format!("parquet open: {e}")))?;
    Ok(builder.schema().metadata().get(PROFILE_KEY).cloned())
}

/// Readiness states from project scanning (Section 6: metadata earns
/// "Ready to add"; a manifest entry is never an eligibility gate).
#[derive(Debug, Clone, PartialEq)]
pub enum CandidateStatus {
    /// Profile metadata present; full validation happens on add/load.
    ReadyToAdd(Box<GroupProfile>),
    /// Parquet without our profile: inspectable, not directly eligible.
    ForeignParquet,
    /// Not a Parquet file at all.
    NotParquet,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScanCandidate {
    pub path: PathBuf,
    pub status: CandidateStatus,
}

/// Recursively discovers Parquet candidates under a project root, skipping
/// dot-directories and the reserved `.archaeodash` transaction area.
pub fn scan_project(root: &Path) -> Result<Vec<ScanCandidate>, ImportError> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| ImportError::Io(e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| ImportError::Io(e.to_string()))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !name.starts_with('.') {
                    stack.push(path);
                }
            } else if name.ends_with(".parquet") {
                let status = match read_profile_metadata(&path) {
                    Ok(Some(json)) => serde_json::from_str::<GroupProfile>(&json)
                        .map(|p| CandidateStatus::ReadyToAdd(Box::new(p)))
                        .unwrap_or(CandidateStatus::ForeignParquet),
                    Ok(None) => CandidateStatus::ForeignParquet,
                    Err(_) => CandidateStatus::NotParquet,
                };
                out.push(ScanCandidate { path, status });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Full validation-on-add (Section 6.6): profile present, version and kind
/// known, roles reference existing columns, identity column is 16-byte binary
/// with unique UUIDs, elemental columns are Float64, and the measured-element
/// checksum matches the stored profile.
pub fn validate_group_file(path: &Path) -> Result<GroupProfile, ImportError> {
    let json = read_profile_metadata(path)?
        .ok_or_else(|| ImportError::Parse("missing archaeodash.profile.v1 metadata".into()))?;
    let profile: GroupProfile = serde_json::from_str(&json)
        .map_err(|e| ImportError::Parse(format!("profile parse: {e}")))?;
    if profile.profile_version != PROFILE_VERSION {
        return Err(ImportError::Parse(format!(
            "unsupported profile version {}",
            profile.profile_version
        )));
    }
    if profile.file_kind != "group" {
        return Err(ImportError::Parse(format!(
            "unknown file_kind {}",
            profile.file_kind
        )));
    }
    let file = File::open(path).map_err(|e| ImportError::Io(e.to_string()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| ImportError::Parse(format!("parquet open: {e}")))?;
    let schema = builder.schema().clone();
    let role_columns = [
        profile.roles.identity.clone(),
        profile.roles.visible_id.clone(),
        profile.roles.legacy_rowid.clone(),
    ];
    for name in role_columns.iter().chain(&profile.roles.descriptive) {
        let field = schema
            .field_with_name(name)
            .map_err(|_| ImportError::Parse(format!("role column {name} missing")))?;
        if *name == profile.roles.identity {
            if field.data_type() != &DataType::FixedSizeBinary(UUID_BYTE_LEN as i32) {
                return Err(ImportError::Parse(
                    "identity column must be 16-byte binary".into(),
                ));
            }
        } else if field.data_type() != &DataType::Utf8 {
            return Err(ImportError::Parse(format!("column {name} must be Utf8")));
        }
    }
    for name in &profile.roles.elemental {
        let field = schema
            .field_with_name(name)
            .map_err(|_| ImportError::Parse(format!("elemental column {name} missing")))?;
        if field.data_type() != &DataType::Float64 {
            return Err(ImportError::Parse(format!(
                "elemental column {name} must be Float64"
            )));
        }
    }
    if schema.fields().len()
        != role_columns.len() + profile.roles.descriptive.len() + profile.roles.elemental.len()
    {
        return Err(ImportError::Parse(
            "schema has columns outside declared roles".into(),
        ));
    }

    let batches: Vec<RecordBatch> = builder
        .build()
        .map_err(|e| ImportError::Parse(format!("parquet read: {e}")))?
        .collect::<Result<Vec<RecordBatch>, _>>()
        .map_err(|e| ImportError::Parse(format!("parquet batch: {e}")))?;
    let mut uuids: Vec<Uuid> = Vec::new();
    let mut elemental_values: Vec<Vec<Option<f64>>> =
        vec![Vec::new(); profile.roles.elemental.len()];
    for batch in &batches {
        let uuid_col = batch
            .column_by_name(IDENTITY_COLUMN)
            .ok_or_else(|| ImportError::Parse("identity column missing from batches".into()))?;
        let uuids_arr = uuid_col
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .ok_or_else(|| ImportError::Parse("identity column wrong type".into()))?;
        for i in 0..uuids_arr.len() {
            let bytes = uuids_arr.value(i);
            if bytes.len() != UUID_BYTE_LEN {
                return Err(ImportError::Parse("identity value is not 16 bytes".into()));
            }
            uuids.push(
                Uuid::from_slice(bytes).map_err(|e| ImportError::Parse(format!("uuid: {e}")))?,
            );
        }
        for (out, name) in elemental_values.iter_mut().zip(&profile.roles.elemental) {
            let col = batch.column_by_name(name).ok_or_else(|| {
                ImportError::Parse(format!("elemental column {name} missing from batch"))
            })?;
            let arr = col
                .as_any()
                .downcast_ref::<Float64Array>()
                .ok_or_else(|| ImportError::Parse(format!("elemental column {name} wrong type")))?;
            for i in 0..arr.len() {
                if arr.is_null(i) {
                    out.push(None);
                } else {
                    out.push(Some(arr.value(i)));
                }
            }
        }
    }
    if uuids.len() != profile.row_count {
        return Err(ImportError::Parse(format!(
            "row count {} does not match profile {}",
            uuids.len(),
            profile.row_count
        )));
    }
    let mut seen = std::collections::HashSet::new();
    for uuid in &uuids {
        if !seen.insert(*uuid) {
            return Err(ImportError::Parse(format!(
                "duplicate analytical_uuid {uuid}"
            )));
        }
    }
    let checksum = measured_elemental_checksum(&uuids, &profile.roles.elemental, &elemental_values);
    if checksum != profile.measured_elemental_checksum {
        return Err(ImportError::Parse(
            "measured elemental checksum mismatch: values changed after write".into(),
        ));
    }
    Ok(profile)
}

/// Reads the hidden `analytical_uuid` identity column from a group file.
/// Used to preserve identities across move/copy/rewrite operations
/// (Section 5.2: the identity is immutable across group operations).
pub fn read_group_uuids(path: &Path) -> Result<Vec<Uuid>, ImportError> {
    let file = File::open(path).map_err(|e| ImportError::Io(e.to_string()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| ImportError::Parse(format!("parquet open: {e}")))?;
    let batches = builder
        .build()
        .map_err(|e| ImportError::Parse(format!("parquet read: {e}")))?
        .collect::<Result<Vec<RecordBatch>, _>>()
        .map_err(|e| ImportError::Parse(format!("parquet batch: {e}")))?;
    let mut uuids = Vec::new();
    for batch in &batches {
        let uuid_col = batch
            .column_by_name(IDENTITY_COLUMN)
            .ok_or_else(|| ImportError::Parse("identity column missing".into()))?;
        let arr = uuid_col
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .ok_or_else(|| ImportError::Parse("identity column wrong type".into()))?;
        for i in 0..arr.len() {
            let bytes = arr.value(i);
            if bytes.len() != UUID_BYTE_LEN {
                return Err(ImportError::Parse("identity value is not 16 bytes".into()));
            }
            uuids.push(
                Uuid::from_slice(bytes).map_err(|e| ImportError::Parse(format!("uuid: {e}")))?,
            );
        }
    }
    Ok(uuids)
}

/// Storage invariant (Section 15.2): no derived-role columns may appear in a
/// group file. Derived analysis values are recomputed on demand and never
/// persisted into profile-conforming files.
pub fn assert_no_derived_columns(profile: &GroupProfile) -> Result<(), ImportError> {
    const DERIVED_PREFIXES: [&str; 9] = [
        "log_",
        "log10_",
        "zscore_",
        "ratio_",
        "imputed_",
        "permuted_",
        "pca_",
        "umap_",
        "lda_",
    ];
    for name in profile
        .roles
        .elemental
        .iter()
        .chain(profile.roles.descriptive.iter())
    {
        let lowered = name.to_lowercase();
        if DERIVED_PREFIXES.iter().any(|p| lowered.starts_with(p)) {
            return Err(ImportError::Parse(format!(
                "column {name} looks like a derived analysis value; group files store measured data only"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_is_order_sensitive_and_stable() {
        let uuids = vec![Uuid::now_v7(), Uuid::now_v7()];
        let cols = vec!["as".to_string(), "la".to_string()];
        let values = vec![vec![Some(1.0), Some(2.0)], vec![None, Some(3.5)]];
        let a = measured_elemental_checksum(&uuids, &cols, &values);
        let b = measured_elemental_checksum(&uuids, &cols, &values);
        assert_eq!(a, b);
        let swapped = vec![vec![Some(2.0), Some(1.0)], vec![None, Some(3.5)]];
        assert_ne!(a, measured_elemental_checksum(&uuids, &cols, &swapped));
    }
}

#[cfg(test)]
mod read_limit_tests {
    use super::*;

    #[test]
    fn footer_preflight_rejects_rows_cells_and_bytes_without_requiring_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("limits.parquet");
        let schema =
            std::sync::Arc::new(Schema::new(vec![Field::new("x", DataType::Float64, false)]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![std::sync::Arc::new(Float64Array::from(vec![1.0, 2.0, 3.0]))],
        )
        .expect("batch");
        let mut writer =
            ArrowWriter::try_new(File::create(&path).expect("file"), schema, None).expect("writer");
        writer.write(&batch).expect("write");
        writer.close().expect("close");
        check_group_read_limits(&path, 3, 3, 1024 * 1024).expect("within limits");
        assert!(matches!(
            check_group_read_limits(&path, 2, 3, 1024 * 1024),
            Err(ImportError::Limit(_))
        ));
        assert!(matches!(
            check_group_read_limits(&path, 3, 2, 1024 * 1024),
            Err(ImportError::Limit(_))
        ));
        assert!(matches!(
            check_group_read_limits(&path, 3, 3, 1),
            Err(ImportError::Limit(_))
        ));
        // Passing a resource preflight cannot make an invalid group file valid.
        assert!(read_group_file(&path).is_err());
    }
}
