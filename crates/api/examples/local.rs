//! Local-only API harness for development and end-to-end verification.
//! Usage: cargo run -p archaeodash-api --example local -- /path/to/project
use archaeodash_api::{root_router, AppState};
use archaeodash_application::{
    AnalysisJobs, ClusterService, ExploreService, ExportService, GroupService, ImportService,
    OrdinationService, PreferenceService, SourceFileService, TransformService,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("provide a project directory")?;
    let state = AppState {
        import: Arc::new(ImportService::new(&root)?),
        groups: Arc::new(GroupService::new(&root)?),
        files: Arc::new(SourceFileService::new(&root)?),
        transforms: Arc::new(TransformService::new(&root)?),
        ordination: Arc::new(OrdinationService::new(&root)?),
        clustering: Arc::new(ClusterService::new(&root)?),
        jobs: Arc::new(AnalysisJobs::new(&root)),
        explore: Arc::new(ExploreService::new(&root)?),
        exports: Arc::new(ExportService::new(&root)?),
        preferences: Arc::new(PreferenceService::new(&root)?),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787").await?;
    eprintln!("Local API listening on http://127.0.0.1:8787");
    axum::serve(listener, root_router(state)).await?;
    Ok(())
}
