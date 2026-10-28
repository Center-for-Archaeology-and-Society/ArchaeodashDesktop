//! Imputation from all groups (`ImputationScope::AllGroups`, the default).
//!
//! Imputation fills a sample's missing values from the other rows, so with
//! the included groups as its rows the imputed values change whenever a
//! group is included or left out. Imputing once over every group in the
//! analyzed folder (the "pool") makes them independent of the selection:
//! each analysis then looks its rows up by sample UUID in the pool's
//! imputed table. Ratios and the transform run afterwards as usual (log and
//! ratios are per sample; z-score still standardizes over the analyzed rows).
//!
//! The table is cached in two layers:
//! - on disk under `.archaeodash/cache/prepared/<content>.json`, keyed by
//!   the imputation settings and every pool row's UUID and measured values
//!   (canonical UUID order), so moving samples between groups reuses it;
//! - in memory, keyed by the pool files' paths, sizes and modification
//!   times, so a request whose pool is unchanged skips reading the pool.
//!
//! Like every ordination cache, it can be deleted at any time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use archaeodash_analysis::{impute, ImputeMethod, MICE_MAXIT};
use archaeodash_contracts::{ImputationMethod, ImputationScope, TransformationDefinition};
use archaeodash_data_io::{read_group_file, GroupFileData};
use archaeodash_domain::DomainError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::combined::{read_locked, CombinedInput};
use crate::transforms::TransformService;

/// Bump when the imputed table's meaning or layout changes.
const PREPARED_CACHE_VERSION: &str = "prepared-imputation-1";
/// Pools kept in memory (one per folder and setup is typical).
const MEMORY_ENTRIES: usize = 8;
/// Imputed tables kept on disk; the oldest go first.
const DISK_ENTRIES: usize = 24;

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

/// The folders whose groups form the imputation pool for a request over
/// `paths`: each path's parent folder (project-relative, `/`-separated;
/// `""` for the project root). Analyses use one folder at a time, so this is
/// normally one folder.
#[derive(Debug, Clone)]
pub struct PoolScope {
    root: PathBuf,
    folders: Vec<String>,
}

impl PoolScope {
    pub fn new<'a>(root: &Path, paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut folders: Vec<String> = Vec::new();
        for path in paths {
            let normalized = path.replace('\\', "/");
            let folder = normalized
                .rsplit_once('/')
                .map_or(String::new(), |(dir, _)| dir.to_string());
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        folders.sort();
        Self {
            root: root.to_path_buf(),
            folders,
        }
    }

    /// A request over `path` plus `extra_paths`.
    pub fn for_request(root: &Path, path: &str, extra_paths: &[String]) -> Self {
        Self::new(
            root,
            std::iter::once(path).chain(extra_paths.iter().map(String::as_str)),
        )
    }
}

/// Whether `definition` imputes, and from all groups (then the pool table
/// replaces imputation over the analyzed rows).
pub fn imputes_from_pool(definition: &TransformationDefinition) -> bool {
    definition.imputation_method != ImputationMethod::None
        && definition.imputation_scope == ImputationScope::AllGroups
        && !definition.elemental_columns.is_empty()
}

/// Imputed values of the definition's elemental columns for every pool
/// sample, by UUID.
#[derive(Debug, Serialize, Deserialize)]
pub struct PooledImputation {
    /// Content key of the pool and settings; part of ordination cache keys
    /// so a change anywhere in the pool computes afresh.
    pub key: String,
    pub columns: Vec<String>,
    uuids: Vec<String>,
    /// Row-major imputed values, `values[i][j]` for `uuids[i]`, `columns[j]`.
    values: Vec<Vec<f64>>,
    #[serde(skip)]
    index: HashMap<String, usize>,
}

impl PooledImputation {
    fn build_index(&mut self) {
        self.index = self
            .uuids
            .iter()
            .enumerate()
            .map(|(i, u)| (u.clone(), i))
            .collect();
    }

    /// Imputed values for one sample (columns in [`Self::columns`] order).
    pub fn row(&self, uuid: &str) -> Option<&[f64]> {
        self.index.get(uuid).map(|&i| self.values[i].as_slice())
    }
}

/// Settings that, with the pool rows, determine the imputed table.
#[derive(Serialize)]
struct ImputationKey<'a> {
    method: ImputationMethod,
    seed: Option<u64>,
    columns: &'a [String],
}

/// Parquet files in the pool folders with their sizes and modification
/// times (project-relative paths, sorted). Cheap: nothing is opened, so this
/// is the fast cache key; which of them are usable groups is decided when
/// the pool is read.
fn pool_listing(pool: &PoolScope) -> Result<Vec<(String, u64, u128)>, DomainError> {
    let mut out = Vec::new();
    for folder in &pool.folders {
        let dir = if folder.is_empty() {
            pool.root.clone()
        } else {
            pool.root.join(folder)
        };
        let entries = std::fs::read_dir(&dir).map_err(|e| DomainError::Internal(Box::new(e)))?;
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !name.to_ascii_lowercase().ends_with(".parquet") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            let rel = if folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            out.push((rel, meta.len(), modified));
        }
    }
    out.sort();
    Ok(out)
}

/// Keeps the newest [`DISK_ENTRIES`] tables (best effort).
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() > DISK_ENTRIES {
        files.sort();
        for (_, old) in &files[..files.len() - DISK_ENTRIES] {
            let _ = std::fs::remove_file(old);
        }
    }
}

/// Content key: settings plus every pool row's UUID and measured values of
/// the imputed columns, in canonical (UUID) order.
fn content_key(settings: &ImputationKey<'_>, data: &GroupFileData, order: &[usize]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(PREPARED_CACHE_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update([0]);
    hasher.update(serde_json::to_vec(settings).unwrap_or_default());
    hasher.update([0]);
    let idx: Vec<Option<usize>> = settings
        .columns
        .iter()
        .map(|c| data.profile.roles.elemental.iter().position(|n| n == c))
        .collect();
    hasher.update((order.len() as u64).to_le_bytes());
    for &i in order {
        let row = &data.rows[i];
        hasher.update(row.uuid.as_bytes());
        for j in &idx {
            match j.and_then(|j| row.elemental.get(j).copied().flatten()) {
                Some(v) => {
                    hasher.update([1]);
                    hasher.update(v.to_bits().to_le_bytes());
                }
                None => hasher.update([0]),
            }
        }
    }
    format!("{:x}", hasher.finalize())
}

/// Recently used pool tables by fast key, oldest first.
type Memory = Mutex<Vec<(String, Arc<PooledImputation>)>>;

fn memory() -> &'static Memory {
    static MEMORY: OnceLock<Memory> = OnceLock::new();
    MEMORY.get_or_init(Default::default)
}

/// Serializes building one pool table at a time per process, so pages and
/// precompute asking together impute once (the rest wait, then hit).
fn build_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
}

fn remember(fast: String, table: Arc<PooledImputation>) {
    let mut entries = memory().lock().unwrap_or_else(PoisonError::into_inner);
    entries.retain(|(k, _)| k != &fast);
    entries.push((fast, table));
    if entries.len() > MEMORY_ENTRIES {
        let excess = entries.len() - MEMORY_ENTRIES;
        entries.drain(..excess);
    }
}

fn recall(fast: &str) -> Option<Arc<PooledImputation>> {
    let entries = memory().lock().unwrap_or_else(PoisonError::into_inner);
    entries
        .iter()
        .find(|(k, _)| k == fast)
        .map(|(_, t)| t.clone())
}

/// The pool's imputed table for `definition` (which must impute from all
/// groups), from memory, the disk cache, or computed and stored.
pub fn pooled_imputation(
    pool: &PoolScope,
    definition: &TransformationDefinition,
    context: &str,
) -> Result<Arc<PooledImputation>, DomainError> {
    let method = match definition.imputation_method {
        ImputationMethod::Pmm => ImputeMethod::Pmm,
        ImputationMethod::MidasTouch => ImputeMethod::MidasTouch,
        ImputationMethod::Rf => ImputeMethod::Rf,
        ImputationMethod::None => {
            return Err(validation("imputation_none", "the setup does not impute"))
        }
    };
    let Some(seed) = definition.imputation_seed else {
        return Err(validation(
            "seed_required",
            "imputation requires a visible, replayable seed (Section 8.3)",
        ));
    };
    let columns = &definition.elemental_columns;
    let settings = ImputationKey {
        method: definition.imputation_method,
        seed: Some(seed),
        columns,
    };

    // Fast key: settings plus the pool files as they are on disk now.
    let files = read_locked(&pool.root, || pool_listing(pool))?;
    let mut fast = Sha256::new();
    fast.update(PREPARED_CACHE_VERSION.as_bytes());
    fast.update(serde_json::to_vec(&settings).unwrap_or_default());
    fast.update(pool.root.to_string_lossy().as_bytes());
    for (rel, len, modified) in &files {
        fast.update(rel.as_bytes());
        fast.update([0]);
        fast.update(len.to_le_bytes());
        fast.update(modified.to_le_bytes());
    }
    let fast = format!("{:x}", fast.finalize());
    if let Some(hit) = recall(&fast) {
        return Ok(hit);
    }

    let _building = build_lock().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(hit) = recall(&fast) {
        return Ok(hit);
    }
    // Group files holding every imputed column; other Parquet files (not
    // groups, or without these elements) cannot contribute.
    let loaded: Vec<(String, GroupFileData)> = read_locked(&pool.root, || {
        Ok(files
            .iter()
            .filter_map(|(rel, _, _)| {
                let data = read_group_file(&pool.root.join(rel)).ok()?;
                columns
                    .iter()
                    .all(|c| data.profile.roles.elemental.contains(c))
                    .then(|| (rel.clone(), data))
            })
            .collect())
    })?;
    if loaded.is_empty() {
        return Err(validation(
            "imputation_pool_empty",
            format!("{context}: no group in the folder has every selected element"),
        ));
    }
    let combined = CombinedInput::from_files(loaded)?;
    let data = combined.data;
    let mut order: Vec<usize> = (0..data.rows.len()).collect();
    order.sort_by(|&a, &b| data.rows[a].uuid.cmp(&data.rows[b].uuid).then(a.cmp(&b)));
    let key = content_key(&settings, &data, &order);
    let file = pool
        .root
        .join(".archaeodash/cache/prepared")
        .join(format!("{key}.json"));

    let cached: Option<PooledImputation> = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let mut table = match cached {
        Some(table) => table,
        None => {
            let canonical = GroupFileData {
                profile: data.profile.clone(),
                rows: order.iter().map(|&i| data.rows[i].clone()).collect(),
            };
            let base = TransformService::matrix_from_group(&canonical, columns, context)?;
            let imputed = impute(&base, method, seed, MICE_MAXIT)
                .map_err(|e| validation("imputation_failed", format!("{context}: {e}")))?
                .matrix;
            let n = canonical.rows.len();
            let table = PooledImputation {
                key: key.clone(),
                columns: imputed.names.clone(),
                uuids: canonical.rows.iter().map(|r| r.uuid.to_string()).collect(),
                values: (0..n)
                    .map(|i| imputed.cols.iter().map(|col| col[i]).collect())
                    .collect(),
                index: HashMap::new(),
            };
            // Best effort, like the ordination cache.
            if let Ok(bytes) = serde_json::to_vec(&table) {
                if archaeodash_data_io::write_atomic(&file, &bytes).is_ok() {
                    if let Some(dir) = file.parent() {
                        prune(dir);
                    }
                }
            }
            table
        }
    };
    table.build_index();
    let table = Arc::new(table);
    remember(fast, table.clone());
    Ok(table)
}

/// The pool table's content key when `definition` imputes from all groups
/// (for ordination cache keys), else `None`.
pub fn pool_key(
    pool: Option<&PoolScope>,
    definition: Option<&TransformationDefinition>,
    context: &str,
) -> Result<Option<String>, DomainError> {
    match (pool, definition) {
        (Some(pool), Some(definition)) if imputes_from_pool(definition) => Ok(Some(
            pooled_imputation(pool, definition, context)?.key.clone(),
        )),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_folders_come_from_request_paths() {
        let pool = PoolScope::for_request(
            Path::new("/p"),
            "Site\\Baca.parquet",
            &["Site/Chaco.parquet".into(), "Top.parquet".into()],
        );
        assert_eq!(pool.folders, vec!["".to_string(), "Site".to_string()]);
    }
}
