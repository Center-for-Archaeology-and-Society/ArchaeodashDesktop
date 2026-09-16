//! CSV/TSV text import, `clean_names(case = "none")` compatibility, and the
//! ArchaeoDash Group Parquet profile (IMPLEMENTATION.md Sections 6.6 and 7).

pub mod clean_names;
pub mod export;
pub mod group_profile;
pub mod loader;
pub mod rnum;

pub use clean_names::{clean_name_case_none, clean_names_case_none, dedupe_names};
pub use export::{csv_field, sync_dir, write_text_csv};
pub use group_profile::{
    assert_no_derived_columns, measured_elemental_checksum, partition_by_group, read_group_file,
    read_group_uuids, read_profile_metadata, sanitize_group_name, scan_project,
    validate_group_file, write_group_file, write_group_rows, CandidateStatus, ColumnRoles,
    GroupFileData, GroupProfile, GroupRow, ImportRecipe, Partition, ScanCandidate, IDENTITY_COLUMN,
    LEGACY_ROWID_COLUMN, PROFILE_KEY, PROFILE_VERSION,
};
pub use loader::{
    data_loader, default_chem_columns, default_id_column, group_partitions,
    guess_numeric_columns_fast, numeric_column, read_text_table, TextFrame,
};
pub use rnum::{compensated_sum, parse_r_numeric, r_format_double, r_round};

/// Typed error for import/export/profile operations.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// Structured parse/validation failure with a stable code prefix.
    #[error("parse error: {0}")]
    Parse(String),
    /// Filesystem or IO failure.
    #[error("io error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke_reports_crate_name() {
        assert_eq!(super::smoke(), "archaeodash-data-io");
    }
}

/// Placeholder smoke function from the Phase 1 skeleton.
pub fn smoke() -> &'static str {
    "archaeodash-data-io"
}
