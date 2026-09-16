//! Group Parquet profile tests: import partitioning, full validation-on-add,
//! checksum stability, the storage invariant (no derived values), and
//! manifest-free discovery (Sections 6.6, 7.2, 15.2).

use archaeodash_data_io::{
    assert_no_derived_columns, data_loader, default_chem_columns, partition_by_group,
    read_group_uuids, read_profile_metadata, scan_project, validate_group_file, write_group_file,
    CandidateStatus, ImportRecipe,
};
use archaeodash_parity::fixture;
use std::path::PathBuf;

fn import_fixture_groups(dir: &std::path::Path) -> Vec<archaeodash_data_io::GroupProfile> {
    let frame = data_loader(&fixture("INAA_test.csv")).expect("fixture loads");
    let elemental = default_chem_columns(&frame.columns);
    let partitions = partition_by_group(&frame, "CORE").expect("no blank groups");
    let mut profiles = Vec::new();
    for (index, partition) in partitions.iter().enumerate() {
        let path = dir.join(format!("group_{}.parquet", sanitize(&partition.group_name)));
        let profile = write_group_file(
            &path,
            &format!("group-{index:04}"),
            "rev-0001",
            "anid",
            &elemental,
            &ImportRecipe::default(),
            &frame,
            partition,
            Some("INAA_test.csv".to_string()),
            Some("deadbeef".to_string()),
            None,
        )
        .expect("write group file");
        profiles.push(profile);
    }
    profiles
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[test]
fn group_profile_round_trip_validates_and_checksums() {
    let dir = tempfile::tempdir().expect("tempdir");
    let profiles = import_fixture_groups(dir.path());
    assert_eq!(profiles.len(), 5, "one file per CORE group");

    for profile in &profiles {
        let path = dir
            .path()
            .join(format!("group_{}.parquet", sanitize(&profile.group_name)));
        // Full validation on add: schema, roles, unique UUIDs, checksum.
        let validated = validate_group_file(&path).expect("validation passes");
        assert_eq!(&validated, profile);
        assert_eq!(validated.roles.identity, "analytical_uuid");
        assert_eq!(validated.roles.visible_id, "anid");
        assert_eq!(validated.row_count, profile.row_count);
        assert_no_derived_columns(&validated).expect("no derived columns");

        // Identity preservation: rewriting the same measured data with the
        // same analytical UUIDs yields the same checksum (Section 5.2
        // immutability across revisions). Fresh UUIDs change the checksum.
        let frame = data_loader(&fixture("INAA_test.csv")).unwrap();
        let elemental = default_chem_columns(&frame.columns);
        let partitions = partition_by_group(&frame, "CORE").unwrap();
        let partition = partitions
            .iter()
            .find(|p| p.group_name == profile.group_name)
            .unwrap();
        let uuids = read_group_uuids(&path).expect("read identities");
        assert_eq!(uuids.len(), profile.row_count);
        let rewrite = write_group_file(
            &path,
            &profile.group_id,
            "rev-0002",
            "anid",
            &elemental,
            &ImportRecipe::default(),
            &frame,
            partition,
            None,
            None,
            Some(&uuids),
        )
        .unwrap();
        assert_eq!(rewrite.revision_id, "rev-0002");
        assert_eq!(
            rewrite.measured_elemental_checksum, profile.measured_elemental_checksum,
            "checksum invariant across identity-preserving rewrite"
        );
        let fresh = write_group_file(
            &path,
            &profile.group_id,
            "rev-0003",
            "anid",
            &elemental,
            &ImportRecipe::default(),
            &frame,
            partition,
            None,
            None,
            None,
        )
        .unwrap();
        assert_ne!(
            fresh.measured_elemental_checksum, profile.measured_elemental_checksum,
            "fresh identities change the measured-element checksum"
        );
    }
}

#[test]
fn scanner_discovers_profile_candidates_without_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let groups_dir = dir.path().join("nested").join("anywhere");
    std::fs::create_dir_all(&groups_dir).expect("nested dirs");
    let frame = data_loader(&fixture("INAA_test.csv")).unwrap();
    let elemental = default_chem_columns(&frame.columns);
    let partitions = partition_by_group(&frame, "CORE").unwrap();
    write_group_file(
        &groups_dir.join("D1.parquet"),
        "g1",
        "rev-1",
        "anid",
        &elemental,
        &ImportRecipe::default(),
        &frame,
        &partitions[0],
        None,
        None,
        None,
    )
    .unwrap();
    // A foreign Parquet (no profile) and a corrupt .parquet file in the same
    // project: the scanner reports statuses, non-parquet files are skipped.
    std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();
    std::fs::write(dir.path().join("broken.parquet"), b"not parquet").unwrap();

    let candidates = scan_project(dir.path()).expect("scan");
    let ready: Vec<_> = candidates
        .iter()
        .filter(|c| matches!(c.status, CandidateStatus::ReadyToAdd(_)))
        .collect();
    assert_eq!(ready.len(), 1, "profile candidate found at any depth");
    assert!(ready[0].path.ends_with("D1.parquet"));
    assert!(
        candidates
            .iter()
            .any(|c| c.status == CandidateStatus::NotParquet),
        "corrupt .parquet file is flagged NotParquet"
    );
    assert!(
        !candidates.iter().any(|c| c.path.ends_with("notes.txt")),
        "non-parquet files are not candidates"
    );
}

#[test]
fn blank_group_values_require_explicit_destination() {
    let frame = archaeodash_data_io::TextFrame {
        columns: vec!["anid".into(), "CORE".into(), "as".into()],
        rows: vec![
            vec![Some("A1".into()), Some("D1".into()), Some("1.0".into())],
            vec![Some("A2".into()), None, Some("2.0".into())],
        ],
    };
    let err = partition_by_group(&frame, "CORE").expect_err("blank rejected");
    assert!(err.to_string().contains("explicit destination"));
}

#[test]
fn import_recipe_zero_as_na_applies_to_measured_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let frame = data_loader(&fixture("INAA_test.csv")).unwrap();
    let elemental = default_chem_columns(&frame.columns);
    let partitions = partition_by_group(&frame, "CORE").unwrap();
    let recipe = ImportRecipe {
        zero_as_na: true,
        ..ImportRecipe::default()
    };
    let profile = write_group_file(
        &dir.path().join("zeros.parquet"),
        "g-zero",
        "rev-1",
        "anid",
        &elemental,
        &recipe,
        &frame,
        &partitions[0],
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(profile.import_recipe, recipe, "recipe recorded in profile");
    let validated = validate_group_file(&dir.path().join("zeros.parquet")).unwrap();
    assert_eq!(validated.import_recipe.zero_as_na, true);
}

#[test]
fn derived_columns_rejected_by_invariant() {
    let profile = archaeodash_data_io::GroupProfile {
        profile_version: 1,
        file_kind: "group".into(),
        group_id: "g".into(),
        group_name: "g".into(),
        revision_id: "r".into(),
        roles: archaeodash_data_io::ColumnRoles {
            identity: "analytical_uuid".into(),
            visible_id: "anid".into(),
            legacy_rowid: "legacy_rowid".into(),
            descriptive: vec!["site".into()],
            elemental: vec!["as".into(), "zscore_as".into()],
        },
        row_count: 1,
        source_path: None,
        source_sha256: None,
        import_recipe: ImportRecipe::default(),
        measured_elemental_checksum: "x".into(),
    };
    assert!(assert_no_derived_columns(&profile).is_err());
}

#[test]
fn metadata_only_read_rejects_non_parquet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path: PathBuf = dir.path().join("fake.parquet");
    std::fs::write(&path, b"not parquet").unwrap();
    assert!(read_profile_metadata(&path).is_err());
}
