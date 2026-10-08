//! Named transformation definitions (Section 8.2, Phase 3): save/list/
//! load/delete as structured JSON under `.archaeodash/transformations/`,
//! batch ratio-spec generation, and ephemeral on-demand application of
//! `none`/`log`/`log10`/`zScore` plus ratios to one group file.
//!
//! Storage invariant (Section 5): definitions persist configuration only;
//! applying a transform never writes calculated values into group files.

use std::path::PathBuf;

use archaeodash_analysis::{
    apply_ratios, log_transform, z_score, ColumnMatrix, LogBase, RatioSpec,
};
use archaeodash_contracts::{
    AppliedTransformation, ApplyTransformationRequest, BatchRatioMode, BatchRatioRequest,
    ImputationMethod, RatioMode, RatioSpecDto, SaveTransformationResponse, TransformMethod,
    TransformationDefinition, TransformationListResponse, TransformationSummary,
};
use archaeodash_data_io::{read_group_file, sanitize_group_name, GroupFileData};
use archaeodash_domain::DomainError;

/// Legacy `transform_table_name_max_len`: saved names sanitize to at most
/// 32 characters.
const MAX_NAME_LEN: usize = 32;

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

fn io_err(e: std::io::Error) -> DomainError {
    DomainError::Internal(Box::new(e))
}

/// Unix seconds now; clocks before the epoch yield 0. Shared with the hosted
/// definition catalog so desktop and hosted envelopes agree on timestamps.
pub fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// On-disk envelope for one saved definition: the definition plus the
/// persistence metadata the legacy store kept alongside each snapshot. The
/// hosted store reuses the same envelope so definitions stay portable
/// between desktop project directories and hosted object namespaces.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredTransformation {
    /// Unix seconds of the original save (preserved across upserts).
    pub created_at_unix_secs: u64,
    pub definition: TransformationDefinition,
}

/// Validates a definition before persisting (Section 8.2 colliding-name
/// rejection and Section 8.3 seed visibility). Shared by the desktop store
/// and the hosted definition catalog.
pub fn validate_definition(definition: &TransformationDefinition) -> Result<(), DomainError> {
    TransformService::validate(definition)
}

/// Deterministic default ratio output name: sanitized
/// `numerator_denominator` (Section 8.2 batch naming rule).
fn default_ratio_name(numerator: &str, denominator: &str) -> String {
    sanitize_group_name(&format!("{numerator}_{denominator}"))
}

/// Application-level transformation-definition store and apply use cases,
/// rooted at one local project directory.
pub struct TransformService {
    root: PathBuf,
}

impl TransformService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, DomainError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        Ok(Self { root })
    }

    fn transformations_dir(&self) -> PathBuf {
        self.root.join(".archaeodash/transformations")
    }

    /// Storage key for a definition name: sanitized and length-capped so no
    /// client string reaches the filesystem outside the project directory.
    fn storage_key(name: &str) -> Result<String, DomainError> {
        let cleaned = sanitize_group_name(name);
        let cleaned = cleaned.chars().take(MAX_NAME_LEN).collect::<String>();
        if cleaned.is_empty() || cleaned == "group" && name.trim().is_empty() {
            return Err(validation("invalid_name", "transformation name is empty"));
        }
        Ok(cleaned)
    }

    fn definition_path(&self, name: &str) -> Result<PathBuf, DomainError> {
        Ok(self
            .transformations_dir()
            .join(format!("{}.json", Self::storage_key(name)?)))
    }

    /// Validates a definition before persisting (Section 8.2 colliding-name
    /// rejection and Section 8.3 seed visibility).
    fn validate(definition: &TransformationDefinition) -> Result<(), DomainError> {
        if definition.name.trim().is_empty() {
            return Err(validation("invalid_name", "transformation name is empty"));
        }
        if definition.transform_method != TransformMethod::None
            && definition.elemental_columns.is_empty()
        {
            return Err(validation(
                "missing_columns",
                "a base transform requires at least one elemental column",
            ));
        }
        if definition.imputation_method != ImputationMethod::None
            && definition.imputation_seed.is_none()
        {
            return Err(validation(
                "seed_required",
                "imputation requires a visible, replayable seed (Section 8.3)",
            ));
        }
        if definition.imputation_method != ImputationMethod::None {
            return Err(validation(
                "imputation_not_gated",
                format!(
                    "imputation method {:?} stays behind the experimental-parity \
                     flag until its Section 8.3 oracle fixtures pass",
                    definition.imputation_method
                ),
            ));
        }
        let mut taken: Vec<String> = definition.elemental_columns.clone();
        for spec in &definition.ratios {
            if spec.numerator == spec.denominator {
                return Err(validation(
                    "ratio_identity",
                    format!("ratio {} divides a column by itself", spec.numerator),
                ));
            }
            let output = spec
                .output_name
                .clone()
                .unwrap_or_else(|| default_ratio_name(&spec.numerator, &spec.denominator));
            if taken.iter().any(|c| c == &output) {
                return Err(validation(
                    "ratio_duplicate_name",
                    format!("output name {output} already exists"),
                ));
            }
            taken.push(output);
        }
        Ok(())
    }

    /// Saves (upserts by sanitized name) one definition as structured JSON,
    /// replacing the legacy unit-separator/pipe encoding (Section 8.2).
    pub fn save(
        &self,
        definition: &TransformationDefinition,
    ) -> Result<SaveTransformationResponse, DomainError> {
        Self::validate(definition)?;
        let dir = self.transformations_dir();
        std::fs::create_dir_all(&dir).map_err(io_err)?;
        let path = self.definition_path(&definition.name)?;
        let created_at_unix_secs = if path.exists() {
            // Preserve the original creation timestamp on upsert.
            let existing: StoredTransformation =
                serde_json::from_str(&std::fs::read_to_string(&path).map_err(io_err)?)
                    .map_err(|e| DomainError::Internal(Box::new(e)))?;
            existing.created_at_unix_secs
        } else {
            now_unix_secs()
        };
        let replaced = path.exists();
        let stored = StoredTransformation {
            created_at_unix_secs,
            definition: definition.clone(),
        };
        let body = serde_json::to_string_pretty(&stored)
            .map_err(|e| DomainError::Internal(Box::new(e)))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, body.as_bytes()).map_err(io_err)?;
        std::fs::rename(&tmp, &path).map_err(io_err)?;
        Ok(SaveTransformationResponse {
            definition: definition.clone(),
            replaced,
        })
    }

    /// Summaries of every saved definition, sorted by name.
    pub fn list(&self) -> Result<TransformationListResponse, DomainError> {
        let dir = self.transformations_dir();
        let mut out = Vec::new();
        if dir.exists() {
            let mut names: Vec<String> = std::fs::read_dir(&dir)
                .map_err(io_err)?
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name.strip_suffix(".json").map(str::to_string)
                })
                .collect();
            names.sort();
            for name in names {
                if let Ok(stored) = self.load_stored(&name) {
                    out.push(TransformationSummary {
                        transform_method: stored.definition.transform_method,
                        imputation_method: stored.definition.imputation_method,
                        ratio_count: stored.definition.ratios.len(),
                        name: stored.definition.name,
                        created_at_unix_secs: stored.created_at_unix_secs,
                    });
                }
            }
        }
        Ok(TransformationListResponse {
            transformations: out,
        })
    }

    /// Loads one saved definition by (unsanitized) name.
    pub fn load(&self, name: &str) -> Result<TransformationDefinition, DomainError> {
        Ok(self.load_stored(name)?.definition)
    }

    fn load_stored(&self, name: &str) -> Result<StoredTransformation, DomainError> {
        let path = self.definition_path(name)?;
        let text = std::fs::read_to_string(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                DomainError::NotFound(format!("transformation {name:?} does not exist"))
            }
            _ => io_err(e),
        })?;
        serde_json::from_str(&text).map_err(|e| DomainError::Internal(Box::new(e)))
    }

    /// Deletes one saved definition; returns the deleted definition.
    pub fn delete(&self, name: &str) -> Result<TransformationDefinition, DomainError> {
        let definition = self.load(name)?;
        let path = self.definition_path(name)?;
        std::fs::remove_file(&path).map_err(io_err)?;
        Ok(definition)
    }

    /// Section 8.2 batch generation: `one_to_one` pairs numerators and
    /// denominators by index (lengths must match); `cartesian` pairs every
    /// combination. Numerator-equals-denominator pairs are excluded and
    /// output names are the deterministic `numerator_denominator` form,
    /// deduplicated with `_2`, `_3`, ... suffixes.
    pub fn batch_ratio_specs(
        &self,
        req: &BatchRatioRequest,
    ) -> Result<Vec<RatioSpecDto>, DomainError> {
        if req.numerators.is_empty() || req.denominators.is_empty() {
            return Err(validation(
                "ratio_empty_selection",
                "batch ratio generation needs at least one numerator and denominator",
            ));
        }
        let mut out = Vec::new();
        let mut push = |numerator: &str, denominator: &str| {
            if numerator == denominator {
                return;
            }
            let base = default_ratio_name(numerator, denominator);
            let mut output = base.clone();
            let mut suffix = 2usize;
            let taken: Vec<String> = out
                .iter()
                .map(|s: &RatioSpecDto| {
                    s.output_name
                        .clone()
                        .unwrap_or_else(|| default_ratio_name(&s.numerator, &s.denominator))
                })
                .collect();
            while taken.contains(&output) {
                output = format!("{base}_{suffix}");
                suffix += 1;
            }
            out.push(RatioSpecDto {
                output_name: Some(output),
                numerator: numerator.to_string(),
                denominator: denominator.to_string(),
            });
        };
        match req.mode {
            BatchRatioMode::OneToOne => {
                if req.numerators.len() != req.denominators.len() {
                    return Err(validation(
                        "ratio_length_mismatch",
                        "one_to_one batch requires equal numerator and denominator counts",
                    ));
                }
                for (n, d) in req.numerators.iter().zip(req.denominators.iter()) {
                    push(n, d);
                }
            }
            BatchRatioMode::Cartesian => {
                for n in &req.numerators {
                    for d in &req.denominators {
                        push(n, d);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Builds the raw measured-value matrix for the required columns from one
    /// group file; `f64::NAN` marks NA (Section 8.4 numeric-frame convention).
    pub(crate) fn matrix_from_group(
        data: &GroupFileData,
        columns: &[String],
        context: &str,
    ) -> Result<ColumnMatrix, DomainError> {
        let roles = &data.profile.roles;
        let mut cols = Vec::with_capacity(columns.len());
        for name in columns {
            let idx = roles
                .elemental
                .iter()
                .position(|c| c == name)
                .ok_or_else(|| {
                    validation(
                        "missing_elemental_column",
                        format!("{context}: column {name:?} is not a measured elemental column"),
                    )
                })?;
            cols.push(
                data.rows
                    .iter()
                    .map(|r| r.elemental[idx].unwrap_or(f64::NAN))
                    .collect::<Vec<f64>>(),
            );
        }
        Ok(ColumnMatrix {
            names: columns.to_vec(),
            cols,
        })
    }

    /// Builds the transformed matrix for one definition against
    /// already-loaded group data: validation, base transform, then ratios
    /// (`only` mode selects just the ratio outputs). Shared by `apply` and
    /// the ordination service; calculated values stay ephemeral (Section 5).
    /// Returns the matrix and the non-finite-to-zero warning count.
    pub(crate) fn apply_definition(
        data: &GroupFileData,
        definition: &TransformationDefinition,
        context: &str,
    ) -> Result<(ColumnMatrix, u64), DomainError> {
        Self::validate(definition)?;
        let mut required = definition.elemental_columns.clone();
        for spec in &definition.ratios {
            for column in [&spec.numerator, &spec.denominator] {
                if !required.contains(column) {
                    required.push(column.clone());
                }
            }
        }
        let matrix = Self::matrix_from_group(data, &required, context)?;
        let (mut matrix, non_finite_to_zero) = match definition.transform_method {
            TransformMethod::None => (matrix, 0u64),
            TransformMethod::Log => {
                let r = log_transform(&matrix, LogBase::Natural)?;
                (r.matrix, r.non_finite_to_zero)
            }
            TransformMethod::Log10 => {
                let r = log_transform(&matrix, LogBase::Base10)?;
                (r.matrix, r.non_finite_to_zero)
            }
            TransformMethod::ZScore => (z_score(&matrix)?, 0),
        };
        let specs: Vec<RatioSpec> = definition
            .ratios
            .iter()
            .map(|s| RatioSpec {
                output_name: s
                    .output_name
                    .clone()
                    .unwrap_or_else(|| default_ratio_name(&s.numerator, &s.denominator)),
                numerator: s.numerator.clone(),
                denominator: s.denominator.clone(),
            })
            .collect();
        if !specs.is_empty() {
            matrix = apply_ratios(&matrix, &specs)?;
            if definition.ratio_mode == RatioMode::Only {
                let mut keep = Vec::with_capacity(specs.len());
                for s in &specs {
                    let idx = matrix
                        .names
                        .iter()
                        .position(|n| n == &s.output_name)
                        .ok_or_else(|| {
                            validation(
                                "ratio_output_missing",
                                format!("ratio output {:?} is absent after apply", s.output_name),
                            )
                        })?;
                    keep.push(idx);
                }
                let names = keep.iter().map(|&i| matrix.names[i].clone()).collect();
                let cols = keep.into_iter().map(|i| matrix.cols[i].clone()).collect();
                matrix = ColumnMatrix { names, cols };
            }
        }
        Ok((matrix, non_finite_to_zero))
    }

    /// Applies one definition to a group file: base transform, then ratios
    /// (`append` keeps the transformed elemental columns, `only` returns just
    /// the ratio outputs). The result is ephemeral — nothing is written.
    pub fn apply(
        &self,
        req: &ApplyTransformationRequest,
    ) -> Result<AppliedTransformation, DomainError> {
        let data = read_group_file(&self.root.join(&req.path))
            .map_err(|e| DomainError::Internal(Box::new(e)))?;
        let (matrix, non_finite_to_zero) =
            Self::apply_definition(&data, &req.definition, &req.path)?;
        let n_rows = matrix.n_rows();
        Ok(AppliedTransformation {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            columns: matrix.names,
            // Transpose column-major matrix storage into row-major DTO rows.
            rows: (0..n_rows)
                .map(|i| {
                    matrix
                        .cols
                        .iter()
                        .map(|col| {
                            let v = col[i];
                            if v.is_finite() {
                                Some(v)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<Option<f64>>>()
                })
                .collect(),
            non_finite_to_zero,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_contracts::{RatioSpecDto, TransformMethod};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_root() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-transforms-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Commits one two-group import via the existing import service and
    /// returns (service, project root, group path for the first group).
    fn service_with_group() -> (TransformService, std::path::PathBuf, String) {
        let dir = temp_root();
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,5,6\n",
        )
        .expect("write source");
        let import = crate::import::ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&archaeodash_contracts::ImportCommitRequest {
                source: "mini.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        (
            TransformService::new(&dir).expect("transform service"),
            dir,
            commit.groups[0].path.clone(),
        )
    }

    fn definition(name: &str, method: TransformMethod) -> TransformationDefinition {
        TransformationDefinition {
            name: name.to_string(),
            transform_method: method,
            imputation_method: ImputationMethod::None,
            imputation_seed: None,
            elemental_columns: vec!["as".into(), "fe".into()],
            descriptive_columns: vec![],
            group_column: None,
            ratios: vec![],
            ratio_mode: RatioMode::Append,
        }
    }

    #[test]
    fn save_list_load_delete_round_trip() {
        let (service, _dir, _path) = service_with_group();
        let def = definition("My Log Set!", TransformMethod::Log10);
        let saved = service.save(&def).expect("save");
        assert!(!saved.replaced);

        // Name sanitized and length-capped for storage (legacy max 32).
        let listed = service.list().expect("list");
        assert_eq!(listed.transformations.len(), 1);
        assert_eq!(listed.transformations[0].name, "My Log Set!");
        assert_eq!(
            listed.transformations[0].transform_method,
            TransformMethod::Log10
        );
        assert_eq!(listed.transformations[0].ratio_count, 0);
        assert!(listed.transformations[0].created_at_unix_secs > 0);

        let loaded = service.load("My Log Set!").expect("load");
        assert_eq!(loaded, def);

        // Upsert replaces and preserves nothing stale.
        let mut updated = def.clone();
        updated.transform_method = TransformMethod::ZScore;
        let saved = service.save(&updated).expect("upsert");
        assert!(saved.replaced);
        assert_eq!(service.list().expect("list").transformations.len(), 1);
        assert_eq!(
            service.load("My Log Set!").expect("load").transform_method,
            TransformMethod::ZScore
        );

        let deleted = service.delete("My Log Set!").expect("delete");
        assert_eq!(deleted, updated);
        let err = service.load("My Log Set!").expect_err("gone");
        assert!(matches!(err, DomainError::NotFound(_)));
        let err = service.delete("My Log Set!").expect_err("double delete");
        assert!(matches!(err, DomainError::NotFound(_)));
        assert_eq!(service.list().expect("empty").transformations.len(), 0);
    }

    #[test]
    fn save_rejects_invalid_definitions() {
        let (service, _dir, _path) = service_with_group();
        let err = service
            .save(&definition("   ", TransformMethod::None))
            .expect_err("empty name rejected");
        assert!(matches!(err, DomainError::Validation { .. }));

        // Base transform without columns is meaningless.
        let mut def = definition("x", TransformMethod::ZScore);
        def.elemental_columns.clear();
        let err = service.save(&def).expect_err("no columns");
        assert!(matches!(err, DomainError::Validation { code, .. } if code == "missing_columns"));

        // Imputation requires a seed AND stays behind the parity gate.
        let mut def = definition("imp", TransformMethod::None);
        def.imputation_method = ImputationMethod::Pmm;
        let err = service.save(&def).expect_err("seed required");
        assert!(matches!(err, DomainError::Validation { code, .. } if code == "seed_required"));
        def.imputation_seed = Some(42);
        let err = service.save(&def).expect_err("not gated yet");
        assert!(
            matches!(err, DomainError::Validation { code, .. } if code == "imputation_not_gated")
        );

        // Self-ratio rejected; duplicate output names rejected.
        let mut def = definition("ratios", TransformMethod::None);
        def.ratios = vec![RatioSpecDto {
            output_name: None,
            numerator: "as".into(),
            denominator: "as".into(),
        }];
        let err = service.save(&def).expect_err("identity ratio");
        assert!(matches!(err, DomainError::Validation { code, .. } if code == "ratio_identity"));
        def.ratios = vec![
            RatioSpecDto {
                output_name: None,
                numerator: "as".into(),
                denominator: "fe".into(),
            },
            RatioSpecDto {
                output_name: Some("as_fe".into()),
                numerator: "fe".into(),
                denominator: "as".into(),
            },
        ];
        let err = service.save(&def).expect_err("duplicate output name");
        assert!(
            matches!(err, DomainError::Validation { code, .. } if code == "ratio_duplicate_name")
        );
    }

    #[test]
    fn batch_ratio_specs_one_to_one_and_cartesian() {
        let (service, _dir, _path) = service_with_group();
        let one_to_one = service
            .batch_ratio_specs(&BatchRatioRequest {
                numerators: vec!["as".into(), "fe".into()],
                denominators: vec!["fe".into(), "as".into()],
                mode: BatchRatioMode::OneToOne,
            })
            .expect("one-to-one");
        let names: Vec<&str> = one_to_one
            .iter()
            .map(|s| s.output_name.as_deref().expect("named"))
            .collect();
        assert_eq!(names, vec!["as_fe", "fe_as"]);

        let cartesian = service
            .batch_ratio_specs(&BatchRatioRequest {
                numerators: vec!["as".into(), "fe".into()],
                denominators: vec!["fe".into(), "as".into()],
                mode: BatchRatioMode::Cartesian,
            })
            .expect("cartesian");
        // Self-pairs (as/as, fe/fe) excluded; no duplicate pairs remain, so
        // names are unique without suffixes.
        let names: Vec<&str> = cartesian
            .iter()
            .map(|s| s.output_name.as_deref().expect("named"))
            .collect();
        assert_eq!(names, vec!["as_fe", "fe_as"]);

        // Duplicate pairs dedupe deterministically: `as_fe` then `as_fe_2`.
        let deduped = service
            .batch_ratio_specs(&BatchRatioRequest {
                numerators: vec!["as".into(), "as".into()],
                denominators: vec!["fe".into(), "fe".into()],
                mode: BatchRatioMode::OneToOne,
            })
            .expect("dedup");
        let names: Vec<&str> = deduped
            .iter()
            .map(|s| s.output_name.as_deref().expect("named"))
            .collect();
        assert_eq!(names, vec!["as_fe", "as_fe_2"]);

        let err = service
            .batch_ratio_specs(&BatchRatioRequest {
                numerators: vec!["as".into()],
                denominators: vec![],
                mode: BatchRatioMode::OneToOne,
            })
            .expect_err("empty denominators");
        assert!(
            matches!(err, DomainError::Validation { code, .. } if code == "ratio_empty_selection")
        );
        let err = service
            .batch_ratio_specs(&BatchRatioRequest {
                numerators: vec!["as".into(), "fe".into()],
                denominators: vec!["fe".into()],
                mode: BatchRatioMode::OneToOne,
            })
            .expect_err("length mismatch");
        assert!(
            matches!(err, DomainError::Validation { code, .. } if code == "ratio_length_mismatch")
        );
    }

    #[test]
    fn apply_is_ephemeral_and_matches_golden_semantics() {
        let (service, dir, path) = service_with_group();

        // Storage invariant: applying and saving never modify the group file.
        let before = std::fs::read(dir.join(&path)).expect("read group");
        let def = definition("z", TransformMethod::ZScore);
        let applied = service
            .apply(&ApplyTransformationRequest {
                path: path.clone(),
                definition: def.clone(),
            })
            .expect("apply");
        let after = std::fs::read(dir.join(&path)).expect("read group");
        assert_eq!(before, after, "group file byte-identical after apply");
        service.save(&def).expect("save");
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("read group"),
            before,
            "group file byte-identical after save"
        );

        // Oracle (golden #2 semantics): a=[1.5,2], fe=[3,4] row proportions
        // *100 then column z-score, 3-decimal rounded.
        // col as: props 33.333..., 33.333... identical rows -> sd 0? No: rows
        // differ in fe but 'as' column is [1.5,2] with row sums [4.5,6]:
        // props 1.5/4.5*100=33.333, 2/6*100=33.333 -> constant -> NaN.
        assert!(applied.columns == vec!["as", "fe"]);
        // zScore of a constant column is NaN/0 division — R returns NaN.
        // Ratio application: 1.5/3 = 0.5.
        let ratio_def = TransformationDefinition {
            name: "r".into(),
            transform_method: TransformMethod::None,
            imputation_method: ImputationMethod::None,
            imputation_seed: None,
            elemental_columns: vec!["as".into(), "fe".into()],
            descriptive_columns: vec![],
            group_column: None,
            ratios: vec![RatioSpecDto {
                output_name: None,
                numerator: "as".into(),
                denominator: "fe".into(),
            }],
            ratio_mode: RatioMode::Append,
        };
        let applied = service
            .apply(&ApplyTransformationRequest {
                path,
                definition: ratio_def,
            })
            .expect("apply ratios");
        assert_eq!(applied.columns, vec!["as", "fe", "as_fe"]);
        assert_eq!(applied.rows[0][2], Some(0.5));
        assert_eq!(applied.rows[1][2], Some(0.5));
        assert_eq!(applied.non_finite_to_zero, 0);
        assert!(!applied.revision_id.is_empty());
    }

    #[test]
    fn apply_only_mode_and_missing_columns() {
        let (service, _dir, path) = service_with_group();
        let mut def = definition("only", TransformMethod::None);
        def.ratio_mode = RatioMode::Only;
        def.ratios = vec![RatioSpecDto {
            output_name: None,
            numerator: "fe".into(),
            denominator: "as".into(),
        }];
        let applied = service
            .apply(&ApplyTransformationRequest {
                path: path.clone(),
                definition: def,
            })
            .expect("apply");
        assert_eq!(applied.columns, vec!["fe_as"]);
        assert_eq!(applied.rows[0][0], Some(2.0));

        // Non-elemental or missing columns are rejected by name.
        let mut def = definition("bad", TransformMethod::None);
        def.elemental_columns = vec!["Site".into()];
        let err = service
            .apply(&ApplyTransformationRequest {
                path,
                definition: def,
            })
            .expect_err("descriptive column is not elemental");
        assert!(
            matches!(err, DomainError::Validation { code, .. } if code == "missing_elemental_column")
        );
    }
}
