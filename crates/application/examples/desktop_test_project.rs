use archaeodash_application::ImportService;
use archaeodash_contracts::{ImportCommitRequest, ImportPreviewRequest};
use std::path::PathBuf;

const FIXTURE: &[u8] = include_bytes!("../../../fixtures/INAA_test.csv");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: desktop_test_project <new-empty-project-directory>")?;
    if destination.exists() {
        return Err(format!("destination already exists: {}", destination.display()).into());
    }
    std::fs::create_dir_all(&destination)?;
    std::fs::write(destination.join("INAA_test.csv"), FIXTURE)?;

    let service = ImportService::new(&destination)?;
    let preview = service.preview(&ImportPreviewRequest {
        source: "INAA_test.csv".into(),
        group_column: Some("CORE".into()),
        group_name: None,
    })?;
    let committed = service.commit(&ImportCommitRequest {
        source: "INAA_test.csv".into(),
        group_column: "CORE".into(),
        visible_id_column: preview.id_column,
        elemental_columns: Some(preview.elemental_columns),
        recipe: None,
        destination_dir: None,
        group_name: None,
    })?;

    println!("Sample project created at {}", destination.display());
    println!(
        "Imported {} rows into {} groups:",
        preview.row_count,
        committed.groups.len()
    );
    for group in committed.groups {
        println!(
            "  {} ({} rows): {}",
            group.group_name, group.row_count, group.path
        );
    }
    Ok(())
}
