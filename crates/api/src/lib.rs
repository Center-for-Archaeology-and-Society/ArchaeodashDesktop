//! Axum HTTP API composition (Section 10).
//!
//! Phase 1 smoke surface (`/healthz`), the Phase 2 local import surface
//! (`POST /api/v1/imports/preview|commit`), the group-operation surface
//! (`GET /api/v1/groups`, `POST /api/v1/groups/validate`,
//! `POST /api/v1/groups/transfer-units`, `POST /api/v1/groups/merge`,
//! `DELETE /api/v1/groups/{*path}`), the local source-file surface
//! (`POST /api/v1/files`, `GET/DELETE /api/v1/files/{id}`,
//! `GET /api/v1/files/{id}/download`), and the Phase 3 transformation
//! surface (`POST/GET /api/v1/transformations`,
//! `GET/DELETE /api/v1/transformations/{name}`,
//! `POST /api/v1/transformations/ratios/batch`,
//! `POST /api/v1/transformations/apply`), all delegating to the shared
//! application use cases. Errors use the transport-neutral
//! problem-details-style `ErrorEnvelope`.

use std::sync::Arc;

use archaeodash_application::{
    app_info, ExploreService, ExportService, GroupService, ImportService, OrdinationService,
    PreferenceService, SourceFileService, TransformService,
};
use archaeodash_contracts::{
    AppInfo, AppliedTransformation, ApplyTransformationRequest, BatchRatioRequest,
    DeleteGroupRequest, DuplicateGroupRequest, ErrorEnvelope, ExploreCompositionalProfileRequest,
    ExploreCompositionalProfileResponse, ExploreCrosstabRequest, ExploreCrosstabResponse,
    ExploreHistogramRequest, ExploreHistogramResponse, ExploreMissingProfileRequest,
    ExploreMissingProfileResponse, ExportMeasuredDataRequest, ExportPcaScoresRequest, ExportResult,
    ExportTransformedRequest, GetPreferencesResponse, GroupCandidate, GroupRowsResponse,
    GroupSummary, ImportCommitRequest, ImportCommitResponse, ImportPreviewRequest,
    ImportPreviewResponse, LdaRequest, LdaResponse, MergeGroupsRequest,
    PatchDescriptiveValuesRequest, PcaRequest, PcaResponse, PutPreferenceRequest,
    SaveTransformationRequest, SaveTransformationResponse, StagedFile, TransactionResponse,
    TransferUnitsRequest, TransformationDefinition, TransformationListResponse, UmapRequest,
    UmapResponse,
};
use archaeodash_data_io::ImportError;
use archaeodash_domain::DomainError;
use archaeodash_storage::StoreError;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};

/// Shared adapter state: one project-scoped import, group, source-file,
/// transformation, ordination, and export service.
#[derive(Clone)]
pub struct AppState {
    pub import: Arc<ImportService>,
    pub groups: Arc<GroupService>,
    pub files: Arc<SourceFileService>,
    pub transforms: Arc<TransformService>,
    pub ordination: Arc<OrdinationService>,
    pub explore: Arc<ExploreService>,
    pub exports: Arc<ExportService>,
    pub preferences: Arc<PreferenceService>,
}

async fn healthz() -> Json<AppInfo> {
    Json(app_info("http", true))
}

/// Maps use-case errors onto HTTP status codes with safe messages
/// (Section 10: RFC 9457-style problem details; no internal diagnostics).
fn error_response(err: ImportError) -> (StatusCode, Json<ErrorEnvelope>) {
    let (status, code) = match &err {
        ImportError::Parse(_) => (StatusCode::UNPROCESSABLE_ENTITY, "parse_error"),
        ImportError::Io(_) => (StatusCode::BAD_REQUEST, "io_error"),
        ImportError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        ImportError::Limit(_) => (StatusCode::PAYLOAD_TOO_LARGE, "limit_exceeded"),
    };
    (
        status,
        Json(ErrorEnvelope {
            code: code.to_string(),
            message: err.to_string(),
        }),
    )
}

/// Maps store errors: stale revisions conflict (409), invariant/schema/
/// validation problems are unprocessable (422), IO failures bad request.
fn store_error_response(err: StoreError) -> (StatusCode, Json<ErrorEnvelope>) {
    let (status, code) = match &err {
        StoreError::RevisionConflict { .. } => (StatusCode::CONFLICT, "revision_conflict"),
        StoreError::SchemaMismatch { .. }
        | StoreError::Invariant(_)
        | StoreError::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation_error"),
        StoreError::Io(_) => (StatusCode::BAD_REQUEST, "io_error"),
    };
    (
        status,
        Json(ErrorEnvelope {
            code: code.to_string(),
            message: err.to_string(),
        }),
    )
}

async fn imports_preview(
    State(state): State<AppState>,
    Json(req): Json<ImportPreviewRequest>,
) -> Result<Json<ImportPreviewResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    // Phase 2 local form: synchronous parse. Large-file streaming moves to
    // the job system in a later phase (Section 12).
    state.import.preview(&req).map(Json).map_err(error_response)
}

async fn imports_commit(
    State(state): State<AppState>,
    Json(req): Json<ImportCommitRequest>,
) -> Result<Json<ImportCommitResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state.import.commit(&req).map(Json).map_err(error_response)
}

async fn groups_scan(
    State(state): State<AppState>,
) -> Result<Json<Vec<GroupCandidate>>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .groups
        .scan_candidates()
        .map(Json)
        .map_err(store_error_response)
}

/// Query parameters for the group dataset read: the project-relative path.
#[derive(Debug, serde::Deserialize)]
struct GroupRowsQuery {
    path: String,
}

/// `GET /api/v1/groups/rows`: full row data of one group file for the client
/// dataset table. The hidden `analytical_uuid` rides in the payload for edit
/// addressing; displaying it is a client-side contract violation (Section 3.2).
async fn groups_rows(
    State(state): State<AppState>,
    Query(query): Query<GroupRowsQuery>,
) -> Result<Json<GroupRowsResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .groups
        .rows(&query.path)
        .map(Json)
        .map_err(store_error_response)
}

async fn groups_validate(
    State(state): State<AppState>,
    Json(path): Json<String>,
) -> Result<Json<GroupSummary>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .groups
        .validate(&path)
        .map(Json)
        .map_err(store_error_response)
}

async fn groups_transfer_units(
    State(state): State<AppState>,
    Json(req): Json<TransferUnitsRequest>,
) -> Result<Json<TransactionResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .groups
        .transfer_units(&req)
        .map(Json)
        .map_err(store_error_response)
}

async fn groups_merge(
    State(state): State<AppState>,
    Json(req): Json<MergeGroupsRequest>,
) -> Result<Json<TransactionResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .groups
        .merge_groups(&req)
        .map(Json)
        .map_err(store_error_response)
}

/// `PATCH /api/v1/groups/descriptive-values`: batch hidden-UUID-addressed
/// descriptive edits in one journaled transaction; elemental columns are
/// locked (Section 4 Phase 4).
async fn groups_patch_descriptive_values(
    State(state): State<AppState>,
    Json(req): Json<PatchDescriptiveValuesRequest>,
) -> Result<Json<TransactionResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .groups
        .patch_descriptive_values(&req)
        .map(Json)
        .map_err(store_error_response)
}

/// `POST /api/v1/groups/duplicate`: duplicate one whole group, preserving
/// analytical UUIDs and lineage by default (Section 10.2).
async fn groups_duplicate(
    State(state): State<AppState>,
    Json(req): Json<DuplicateGroupRequest>,
) -> Result<Json<TransactionResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .groups
        .duplicate_group(&req)
        .map(Json)
        .map_err(store_error_response)
}

/// Query parameters for the destructive delete route (Section 10.4: the
/// client confirms the exact path and names the revision it last read).
#[derive(Debug, serde::Deserialize)]
struct DeleteGroupQuery {
    expected_revision: String,
    confirm_path: String,
}

/// `DELETE /api/v1/groups/{*path}`: journaled group deletion guarded by
/// exact-path confirmation and optimistic concurrency on the revision.
async fn groups_delete(
    State(state): State<AppState>,
    Path(path): Path<String>,
    Query(query): Query<DeleteGroupQuery>,
) -> Result<Json<TransactionResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .groups
        .delete_group(&DeleteGroupRequest {
            path,
            expected_revision: query.expected_revision,
            confirm_path: query.confirm_path,
        })
        .map(Json)
        .map_err(store_error_response)
}

/// Query parameters for the local upload route: the user-selected
/// in-project logical path (Section 10.2 `POST /projects/{id}/files`; the
/// hosted form scopes the path by project ID instead).
#[derive(Debug, serde::Deserialize)]
struct UploadFileQuery {
    path: String,
}

/// Local Phase-2 upload: bytes arrive as the raw request body, stage through
/// the bounded quarantine, and promote to the logical path.
async fn files_upload(
    State(state): State<AppState>,
    Query(query): Query<UploadFileQuery>,
    bytes: Bytes,
) -> Result<Json<StagedFile>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .files
        .upload(&query.path, &bytes)
        .map(Json)
        .map_err(error_response)
}

/// `GET /api/v1/files/{id}`: quarantine-record metadata including checksum,
/// format, parse state, and soft-delete tombstone.
async fn files_metadata(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<StagedFile>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .files
        .metadata(&file_id)
        .map(Json)
        .map_err(error_response)
}

/// `GET /api/v1/files/{id}/download`: raw bytes with a safe display
/// filename derived from the recorded logical path.
async fn files_download(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorEnvelope>)> {
    let download = state.files.download(&file_id).map_err(error_response)?;
    let filename: String = download
        .metadata
        .path
        .rsplit('/')
        .next()
        .unwrap_or("download.bin")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-_. ".contains(*c))
        .collect();
    let filename = if filename.is_empty() {
        "download.bin".to_string()
    } else {
        filename
    };
    let mut response = (StatusCode::OK, download.content).into_response();
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")).map_err(
            |_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorEnvelope {
                        code: "header_error".to_string(),
                        message: "could not build download header".to_string(),
                    }),
                )
            },
        )?,
    );
    Ok(response)
}

/// `DELETE /api/v1/files/{id}`: soft delete — bytes move to quarantine
/// trash, the tombstoned record remains readable.
async fn files_delete(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<StagedFile>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .files
        .delete(&file_id)
        .map(Json)
        .map_err(error_response)
}

/// Maps domain errors: validation/identity problems are unprocessable (422),
/// not-found is 404, and internal failures stay opaque (500).
fn domain_error_response(err: DomainError) -> (StatusCode, Json<ErrorEnvelope>) {
    let (status, code) = match &err {
        DomainError::InvalidIdentity { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid_identity")
        }
        DomainError::Validation { .. } => (StatusCode::UNPROCESSABLE_ENTITY, "validation_error"),
        DomainError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        DomainError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    (
        status,
        Json(ErrorEnvelope {
            code: code.to_string(),
            message: err.to_string(),
        }),
    )
}

/// `POST /api/v1/transformations`: save (upsert by name) one definition.
async fn transformations_save(
    State(state): State<AppState>,
    Json(req): Json<SaveTransformationRequest>,
) -> Result<Json<SaveTransformationResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .transforms
        .save(&req.definition)
        .map(Json)
        .map_err(domain_error_response)
}

/// `GET /api/v1/transformations`: summaries sorted by name.
async fn transformations_list(
    State(state): State<AppState>,
) -> Result<Json<TransformationListResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .transforms
        .list()
        .map(Json)
        .map_err(domain_error_response)
}

/// `GET /api/v1/transformations/{name}`: one full definition.
async fn transformations_load(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<TransformationDefinition>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .transforms
        .load(&name)
        .map(Json)
        .map_err(domain_error_response)
}

/// `DELETE /api/v1/transformations/{name}`: removes the saved definition.
async fn transformations_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<TransformationDefinition>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .transforms
        .delete(&name)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/transformations/ratios/batch`: one-to-one or Cartesian
/// ratio-spec generation (Section 8.2).
async fn transformations_batch_ratios(
    State(state): State<AppState>,
    Json(req): Json<BatchRatioRequest>,
) -> Result<Json<Vec<archaeodash_contracts::RatioSpecDto>>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .transforms
        .batch_ratio_specs(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/transformations/apply`: ephemeral on-demand application;
/// never persists calculated values (Section 5 storage invariant).
async fn transformations_apply(
    State(state): State<AppState>,
    Json(req): Json<ApplyTransformationRequest>,
) -> Result<Json<AppliedTransformation>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .transforms
        .apply(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/ordination/pca`: prcomp-parity PCA over one group file;
/// results are ephemeral (Section 5 storage invariant).
async fn ordination_pca(
    State(state): State<AppState>,
    Json(req): Json<PcaRequest>,
) -> Result<Json<PcaResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .ordination
        .pca(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/ordination/lda`: `MASS::lda` moment-method parity over one
/// group file with the legacy three-group gate.
async fn ordination_lda(
    State(state): State<AppState>,
    Json(req): Json<LdaRequest>,
) -> Result<Json<LdaResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .ordination
        .lda(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/ordination/umap`: legacy `umap::umap(method = "naive")`
/// parity over one group file with a deterministic seed; results are
/// ephemeral (Section 5 storage invariant).
async fn ordination_umap(
    State(state): State<AppState>,
    Json(req): Json<UmapRequest>,
) -> Result<Json<UmapResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .ordination
        .umap(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/explore/missing-profile`: `profile_missing` band summary;
/// results are ephemeral (Section 5 storage invariant).
async fn explore_missing_profile(
    State(state): State<AppState>,
    Json(req): Json<ExploreMissingProfileRequest>,
) -> Result<Json<ExploreMissingProfileResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .explore
        .missing_profile(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/explore/histogram`: `hist.default` breakpoints and counts.
async fn explore_histogram(
    State(state): State<AppState>,
    Json(req): Json<ExploreHistogramRequest>,
) -> Result<Json<ExploreHistogramResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .explore
        .histogram(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/explore/crosstab`: legacy `compute_crosstab_summary`.
async fn explore_crosstab(
    State(state): State<AppState>,
    Json(req): Json<ExploreCrosstabRequest>,
) -> Result<Json<ExploreCrosstabResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .explore
        .crosstab(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/explore/compositional-profile`: the `pivot_longer` long
/// table, optionally grouped.
async fn explore_compositional_profile(
    State(state): State<AppState>,
    Json(req): Json<ExploreCompositionalProfileRequest>,
) -> Result<Json<ExploreCompositionalProfileResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .explore
        .compositional_profile(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/exports/measured-data`: the measured chemical frame of one
/// group file as ephemeral CSV (Section 7.3; legacy `rvals$selectedData`).
/// Nothing is persisted; the client saves the returned content.
async fn exports_measured_data(
    State(state): State<AppState>,
    Json(req): Json<ExportMeasuredDataRequest>,
) -> Result<Json<ExportResult>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .exports
        .export_measured_data(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/exports/transformed`: the explicitly computed transformed
/// result as ephemeral CSV (Section 7.3); calculated values are never
/// persisted into group files (Section 5 storage invariant).
async fn exports_transformed(
    State(state): State<AppState>,
    Json(req): Json<ExportTransformedRequest>,
) -> Result<Json<ExportResult>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .exports
        .export_transformed(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `POST /api/v1/exports/pca-scores`: the computed PCA score frame (`pcadf`
/// equivalent, Section 3.2 correction of the legacy `rvals$pcaData` bug) as
/// ephemeral CSV.
async fn exports_pca_scores(
    State(state): State<AppState>,
    Json(req): Json<ExportPcaScoresRequest>,
) -> Result<Json<ExportResult>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .exports
        .export_pca_scores(&req)
        .map(Json)
        .map_err(domain_error_response)
}

/// `GET /api/v1/preferences`: every stored preference (absent keys read as
/// defaults client-side). Desktop file store now; hosted per-user
/// control-plane rows in Phase 7 (Section 6.5).
async fn preferences_get(
    State(state): State<AppState>,
) -> Result<Json<GetPreferencesResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .preferences
        .get_all()
        .map(Json)
        .map_err(domain_error_response)
}

/// `PUT /api/v1/preferences`: upsert one allowlisted, shape-validated
/// preference (Section 10.1; legacy `write_user_preference_safe` upsert
/// semantics, typed).
async fn preferences_set(
    State(state): State<AppState>,
    Json(req): Json<PutPreferenceRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
    state
        .preferences
        .set(&req)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(domain_error_response)
}

/// Builds the root router. Route groups for analysis, jobs, and auth land in
/// their owning phases (Sections 10.1+).
pub fn root_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/imports/preview", post(imports_preview))
        .route("/api/v1/imports/commit", post(imports_commit))
        .route("/api/v1/files", post(files_upload))
        .route(
            "/api/v1/files/{id}",
            get(files_metadata).delete(files_delete),
        )
        .route("/api/v1/files/{id}/download", get(files_download))
        .route("/api/v1/groups", get(groups_scan))
        .route("/api/v1/groups/validate", post(groups_validate))
        .route("/api/v1/groups/rows", get(groups_rows))
        .route("/api/v1/groups/transfer-units", post(groups_transfer_units))
        .route("/api/v1/groups/merge", post(groups_merge))
        .route(
            "/api/v1/groups/descriptive-values",
            patch(groups_patch_descriptive_values),
        )
        .route("/api/v1/groups/duplicate", post(groups_duplicate))
        .route("/api/v1/groups/{*path}", delete(groups_delete))
        .route(
            "/api/v1/transformations",
            post(transformations_save).get(transformations_list),
        )
        .route(
            "/api/v1/transformations/{name}",
            get(transformations_load).delete(transformations_delete),
        )
        .route(
            "/api/v1/transformations/ratios/batch",
            post(transformations_batch_ratios),
        )
        .route("/api/v1/transformations/apply", post(transformations_apply))
        .route("/api/v1/ordination/pca", post(ordination_pca))
        .route("/api/v1/ordination/lda", post(ordination_lda))
        .route("/api/v1/ordination/umap", post(ordination_umap))
        .route("/api/v1/exports/measured-data", post(exports_measured_data))
        .route("/api/v1/exports/transformed", post(exports_transformed))
        .route("/api/v1/exports/pca-scores", post(exports_pca_scores))
        .route(
            "/api/v1/preferences",
            get(preferences_get).put(preferences_set),
        )
        .route(
            "/api/v1/explore/missing-profile",
            post(explore_missing_profile),
        )
        .route("/api/v1/explore/histogram", post(explore_histogram))
        .route("/api/v1/explore/crosstab", post(explore_crosstab))
        .route(
            "/api/v1/explore/compositional-profile",
            post(explore_compositional_profile),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_contracts::{
        BatchRatioMode, CrosstabRows, DescriptiveEdit, DuplicateGroupRequest, GroupRowsResponse,
        ImputationMethod, PatchDescriptiveValuesRequest, RatioMode, RatioSpecDto, TransferAction,
        TransformMethod,
    };
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// JSON body helper for POST requests.
    fn json_body<T: serde::Serialize>(value: &T) -> Body {
        Body::from(serde_json::to_vec(value).expect("serialize"))
    }

    fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState {
            import: Arc::new(ImportService::new(dir.path()).expect("service")),
            groups: Arc::new(GroupService::new(dir.path()).expect("group service")),
            files: Arc::new(SourceFileService::new(dir.path()).expect("file service")),
            transforms: Arc::new(TransformService::new(dir.path()).expect("transform service")),
            ordination: Arc::new(OrdinationService::new(dir.path()).expect("ordination service")),
            explore: Arc::new(ExploreService::new(dir.path()).expect("explore service")),
            exports: Arc::new(ExportService::new(dir.path()).expect("export service")),
            preferences: Arc::new(PreferenceService::new(dir.path()).expect("preference service")),
        };
        (state, dir)
    }

    /// Commits the two-group fixture over HTTP and returns the response.
    async fn commit_fixture(app: axum::Router, dir: &tempfile::TempDir) -> ImportCommitResponse {
        std::fs::write(
            dir.path().join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,5,6\n",
        )
        .expect("write source");
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/imports/commit")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ImportCommitRequest {
                            source: "mini.csv".into(),
                            group_column: "Site".into(),
                            visible_id_column: None,
                            elemental_columns: None,
                            recipe: None,
                            destination_dir: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn healthz_reports_ready_over_http() {
        let (state, _dir) = test_state();
        let response = root_router(state)
            .oneshot(
                axum::http::Request::get("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let info: AppInfo = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(info.transport, "http");
        assert!(info.ready);
    }

    #[tokio::test]
    async fn preview_and_commit_round_trip_over_http() {
        let (state, dir) = test_state();
        std::fs::write(
            dir.path().join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,,6\n",
        )
        .expect("write source");
        let app = root_router(state);

        // Preview: defaults plus partition summary, source untouched.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/imports/preview")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ImportPreviewRequest {
                            source: "mini.csv".into(),
                            group_column: Some("Site".into()),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let preview: ImportPreviewResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(preview.row_count, 3);
        assert_eq!(preview.id_column.as_deref(), Some("anid"));
        assert_eq!(preview.partitions.len(), 2);
        assert!(
            !dir.path().join("groups").exists(),
            "preview writes nothing"
        );

        // Commit: one validated group file per partition.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/imports/commit")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ImportCommitRequest {
                            source: "mini.csv".into(),
                            group_column: "Site".into(),
                            visible_id_column: None,
                            elemental_columns: None,
                            recipe: None,
                            destination_dir: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let commit: ImportCommitResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(commit.groups.len(), 2);
        assert_eq!(commit.groups[0].path, "groups/Baca.parquet");
        assert!(dir.path().join("groups/Baca.parquet").exists());
        assert!(dir.path().join("groups/Hooper.parquet").exists());
    }

    #[tokio::test]
    async fn group_scan_validate_and_transfer_over_http() {
        let (state, dir) = test_state();
        let app = root_router(state);
        let commit = commit_fixture(app.clone(), &dir).await;

        // Scan lists both ready candidates.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/groups")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let candidates: Vec<GroupCandidate> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|c| c.ready));

        // Validate returns the summary for one path.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/groups/validate")
                    .header("content-type", "application/json")
                    .body(Body::from("\"groups/Baca.parquet\""))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let summary: GroupSummary = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(summary.group_name, "Baca");
        assert_eq!(summary.row_count, 2);

        // Transfer with the revision just validated. Summaries intentionally
        // omit hidden identities (Section 10.2), so the HTTP tests exercise
        // rejection paths; real transfers run at the application layer.
        let source_path = commit.groups[0].path.clone();
        let baca_revision = summary.revision_id;
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/groups/transfer-units")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&TransferUnitsRequest {
                            action: TransferAction::Move,
                            source_path: source_path.clone(),
                            destination_path: commit.groups[1].path.clone(),
                            destination_group_name: None,
                            selected_uuids: vec!["not-a-uuid".into()],
                            expected_source_revision: baca_revision.clone(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.code, "validation_error");

        // Stale revision conflicts (409) before any file is touched.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/groups/transfer-units")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&TransferUnitsRequest {
                            action: TransferAction::Move,
                            source_path: source_path.clone(),
                            destination_path: commit.groups[1].path.clone(),
                            destination_group_name: None,
                            selected_uuids: vec![uuid::Uuid::now_v7().to_string()],
                            expected_source_revision: "rev-999".into(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);

        // Merge both groups into one through the journaled transaction.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/groups/merge")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&MergeGroupsRequest {
                            sources: commit.groups.iter().map(|g| g.path.clone()).collect(),
                            new_group_name: "Merged".into(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let tx: TransactionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(tx.action, "merge_groups");
        assert_eq!(tx.outputs.len(), 1);
        assert_eq!(tx.outputs[0].row_count, 3);
        assert_eq!(tx.deleted_paths, vec![commit.groups[1].path.clone()]);
        assert!(!dir.path().join(&commit.groups[1].path).exists());
    }

    #[tokio::test]
    async fn group_rows_route_returns_full_dataset_with_hidden_uuids() {
        let (state, dir) = test_state();
        let app = root_router(state);
        let _commit = commit_fixture(app.clone(), &dir).await;

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/groups/rows?path=groups/Baca.parquet")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let rows: GroupRowsResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(rows.path, "groups/Baca.parquet");
        assert_eq!(rows.visible_id_column, "anid");
        assert_eq!(rows.descriptive_columns, vec!["Site"]);
        assert_eq!(rows.elemental_columns, vec!["as", "fe"]);
        assert_eq!(rows.rows.len(), 2);
        // Hidden identity rides in the payload for edit addressing; visible ID
        // and descriptive cells round-trip; measured values stay numeric.
        assert!(rows
            .rows
            .iter()
            .all(|row| uuid::Uuid::parse_str(&row.analytical_uuid).is_ok()));
        assert_eq!(rows.rows[0].visible_id.as_deref(), Some("A1"));
        assert_eq!(rows.rows[0].descriptive, vec![Some("Baca".into())]);
        assert_eq!(rows.rows[0].elemental, vec![Some(1.5), Some(3.0)]);

        // Unknown paths fail closed with the store error envelope (missing
        // group files surface as io_error, per store_error_response).
        let response = app
            .oneshot(
                axum::http::Request::get("/api/v1/groups/rows?path=groups/Missing.parquet")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.code, "io_error");
    }

    #[tokio::test]
    async fn delete_group_route_confirms_revisions_and_removes_file() {
        let (state, dir) = test_state();
        let app = root_router(state);
        let commit = commit_fixture(app.clone(), &dir).await;
        let path = &commit.groups[0].path;

        // Mismatched exact-path confirmation is rejected (422) and nothing is
        // deleted.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::delete(&format!("/api/v1/groups/{path}?expected_revision=rev-1&confirm_path=groups/Other.parquet"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        assert!(dir.path().join(path).exists());

        // Stale revision conflicts (409) before any file is touched.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::delete(&format!(
                    "/api/v1/groups/{path}?expected_revision=rev-999&confirm_path={path}"
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        assert!(dir.path().join(path).exists());

        // Confirmed delete succeeds and removes the file.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::delete(&format!(
                    "/api/v1/groups/{path}?expected_revision=rev-1&confirm_path={path}"
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let tx: TransactionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(tx.action, "delete_group");
        assert_eq!(tx.deleted_paths, vec![path.clone()]);
        assert!(!dir.path().join(path).exists());

        // Deleting again now fails: the file no longer validates as a group.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::delete(&format!(
                    "/api/v1/groups/{path}?expected_revision=rev-1&confirm_path={path}"
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn file_upload_metadata_download_delete_round_trip() {
        let (state, dir) = test_state();
        let app = root_router(state);

        // Upload stages through quarantine and promotes to the logical path.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/files?path=sources%2Fmini.csv")
                    .header("content-type", "application/octet-stream")
                    .body(Body::from("anid,Site,as\nA1,Baca,1.5\nA2,Baca,2\n"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let staged: StagedFile = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(staged.path, "sources/mini.csv");
        assert_eq!(staged.format, "csv");
        assert_eq!(staged.parse_state, "parsed");
        assert!(!staged.deleted);
        assert!(dir.path().join("sources/mini.csv").exists());

        // Metadata by ID.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get(format!("/api/v1/files/{}", staged.file_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let meta: StagedFile = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(meta, staged);

        // Download returns the exact uploaded bytes and a safe filename.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get(format!("/api/v1/files/{}/download", staged.file_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("content-disposition")
                .and_then(|v| v.to_str().ok()),
            Some("attachment; filename=\"mini.csv\"")
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes.as_ref(), b"anid,Site,as\nA1,Baca,1.5\nA2,Baca,2\n");

        // Soft delete tombstones the record and removes the logical file.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::delete(format!("/api/v1/files/{}", staged.file_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let deleted: StagedFile = serde_json::from_slice(&bytes).unwrap();
        assert!(deleted.deleted);
        assert!(!dir.path().join("sources/mini.csv").exists());

        // Download of a deleted file is 404; re-upload to the same path is
        // rejected until the tombstone flow is replaced by the hosted catalog.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get(format!("/api/v1/files/{}/download", staged.file_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/files?path=sources%2Fmini.csv")
                    .header("content-type", "application/octet-stream")
                    .body(Body::from("anid,Site,as\nA1,Baca,1\n"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn file_upload_rejects_escape_bad_format_and_oversize() {
        let (state, _dir) = test_state();
        let app = root_router(state);

        // Path escape: 422 parse_error.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/files?path=..%2Foutside.csv")
                    .header("content-type", "application/octet-stream")
                    .body(Body::from("x"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );

        // Non-allowlisted extension: 422.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/files?path=sources%2Fthing.exe")
                    .header("content-type", "application/octet-stream")
                    .body(Body::from("x"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.code, "parse_error");

        // Unknown file ID: 404.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/files/01900000-0000-7000-8000-00000000000f")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn transformation_routes_save_apply_list_delete() {
        let (state, dir) = test_state();
        let app = root_router(state);

        // Commit the fixture so apply has a group file to transform.
        let commit = commit_fixture(app.clone(), &dir).await;
        let group_path = commit.groups[0].path.clone();

        // Save a definition (upsert semantics).
        let definition = TransformationDefinition {
            name: "log ratios".into(),
            transform_method: TransformMethod::Log10,
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
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/transformations")
                    .header("content-type", "application/json")
                    .body(json_body(&SaveTransformationRequest {
                        definition: definition.clone(),
                    }))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let saved: SaveTransformationResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(!saved.replaced);

        // List shows one summary.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/transformations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let listed: TransformationListResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(listed.transformations.len(), 1);
        assert_eq!(listed.transformations[0].name, "log ratios");
        assert_eq!(
            listed.transformations[0].transform_method,
            TransformMethod::Log10
        );

        // Load by name round-trips the definition.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/transformations/log%20ratios")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let loaded: TransformationDefinition = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(loaded, definition);

        // Apply the saved definition to the committed group file: ephemeral
        // result with log10 + ratio columns, group file untouched on disk.
        let before = std::fs::read(dir.path().join(&group_path)).expect("read group");
        let apply = ApplyTransformationRequest {
            path: group_path.clone(),
            definition: definition.clone(),
        };
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/transformations/apply")
                    .header("content-type", "application/json")
                    .body(json_body(&apply))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let applied: AppliedTransformation = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(applied.path, group_path);
        assert_eq!(applied.columns, vec!["as", "fe", "as_fe"]);
        assert_eq!(applied.rows.len(), commit.groups[0].row_count as usize);
        assert!(applied.rows.iter().flatten().all(|v| v.is_some()));
        assert_eq!(
            std::fs::read(dir.path().join(&group_path)).expect("read group"),
            before,
            "group file byte-identical after apply"
        );

        // Batch ratio generation.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/transformations/ratios/batch")
                    .header("content-type", "application/json")
                    .body(json_body(&BatchRatioRequest {
                        numerators: vec!["as".into()],
                        denominators: vec!["fe".into()],
                        mode: BatchRatioMode::OneToOne,
                    }))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let specs: Vec<archaeodash_contracts::RatioSpecDto> =
            serde_json::from_slice(&bytes).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].output_name.as_deref(), Some("as_fe"));

        // Validation failures are 422 with a stable code.
        let mut bad = definition.clone();
        bad.ratios = vec![RatioSpecDto {
            output_name: None,
            numerator: "as".into(),
            denominator: "as".into(),
        }];
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/transformations")
                    .header("content-type", "application/json")
                    .body(json_body(&SaveTransformationRequest { definition: bad }))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );

        // Unknown name load is 404; delete then load again is 404.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/transformations/nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::delete("/api/v1/transformations/log%20ratios")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/transformations/log%20ratios")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn preview_path_escape_returns_unprocessable_entity() {
        let (state, _dir) = test_state();
        let response = root_router(state)
            .oneshot(
                axum::http::Request::post("/api/v1/imports/preview")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ImportPreviewRequest {
                            source: "../outside.csv".into(),
                            group_column: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.code, "parse_error");
    }

    /// Phase 4 ordination surface: PCA over one committed group file returns
    /// ephemeral results; a bad column is a 422 validation error.
    #[tokio::test]
    async fn ordination_pca_round_trip_and_validation_error() {
        let (state, dir) = test_state();
        let app = root_router(state.clone());
        let commit = commit_fixture(app.clone(), &dir).await;
        let path = commit.groups[0].path.clone();

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/ordination/pca")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&PcaRequest {
                            path: path.clone(),
                            columns: vec!["as".into(), "fe".into()],
                            scale: false,
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let pca: PcaResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(pca.path, path);
        assert_eq!(pca.score_names, vec!["PC1", "PC2"]);
        assert_eq!(pca.scores.len(), commit.groups[0].row_count as usize);
        assert_eq!(pca.rotation.len(), 2);
        assert!(pca.scale.is_none());

        // Missing elemental column -> 422 validation envelope.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/ordination/pca")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&PcaRequest {
                            path,
                            columns: vec!["cu".into()],
                            scale: false,
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.code, "validation_error");
    }

    /// LDA over a merged three-group file through the HTTP adapter, plus the
    /// legacy three-group minimum gate as a 422.
    #[tokio::test]
    async fn ordination_lda_round_trip_and_group_gate() {
        let (state, dir) = test_state();
        let app = root_router(state.clone());
        // Three groups so the merged file passes the legacy minimum.
        std::fs::write(
            dir.path().join("three.csv"),
            "anid,Site,as,fe\nA1,A,1.5,3\nA2,A,2,4\nB1,B,5,6\nB2,B,6,8\nC1,C,9,1\nC2,C,11,2\n",
        )
        .expect("write source");
        let import_response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/imports/commit")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ImportCommitRequest {
                            source: "three.csv".into(),
                            group_column: "Site".into(),
                            visible_id_column: None,
                            elemental_columns: None,
                            recipe: None,
                            destination_dir: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(import_response.status(), axum::http::StatusCode::OK);
        let bytes = import_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let import: ImportCommitResponse = serde_json::from_slice(&bytes).unwrap();
        let sources: Vec<String> = import.groups.iter().map(|g| g.path.clone()).collect();
        let merge_response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/groups/merge")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&MergeGroupsRequest {
                            sources,
                            new_group_name: "Merged".into(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(merge_response.status(), axum::http::StatusCode::OK);
        let bytes = merge_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let merge: TransactionResponse = serde_json::from_slice(&bytes).unwrap();
        let merged_path = merge.outputs[0].path.clone();
        let merged_rows = merge.outputs[0].row_count;

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/ordination/lda")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&LdaRequest {
                            path: merged_path,
                            columns: vec!["as".into(), "fe".into()],
                            group_column: "Site".into(),
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let lda: LdaResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(lda.levels.len(), 3);
        assert_eq!(lda.score_names, vec!["LD1", "LD2"]);
        assert_eq!(lda.scores.len(), merged_rows as usize);

        // Two visible groups cannot support LDA: 422 with the legacy gate.
        // Merge the two-group fixture into one file spanning both levels.
        let pair_commit = commit_fixture(app.clone(), &dir).await;
        let pair_sources: Vec<String> = pair_commit.groups.iter().map(|g| g.path.clone()).collect();
        let pair_merge_response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/groups/merge")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&MergeGroupsRequest {
                            sources: pair_sources,
                            new_group_name: "Pair".into(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pair_merge_response.status(), axum::http::StatusCode::OK);
        let bytes = pair_merge_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let pair_merge: TransactionResponse = serde_json::from_slice(&bytes).unwrap();
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/ordination/lda")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&LdaRequest {
                            path: pair_merge.outputs[0].path.clone(),
                            columns: vec!["as".into(), "fe".into()],
                            group_column: "Site".into(),
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            status,
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            "gate body: {}",
            String::from_utf8_lossy(&bytes)
        );
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert!(envelope.message.contains("LDA requires at least 3 groups"));
    }

    /// UMAP over one committed group file through the HTTP adapter: seeded
    /// deterministic embedding, legacy config echo, and the empty-column and
    /// too-few-rows validation gates as 422s.
    #[tokio::test]
    async fn ordination_umap_round_trip_and_validation_error() {
        let (state, dir) = test_state();
        let app = root_router(state.clone());
        // UMAP's legacy n_neighbors = 15 needs more than 15 rows.
        let rows: Vec<String> = (0..20)
            .map(|i| {
                let v = i as f64;
                format!("A{i},A,{},{}", 1.5 + v, 3.0 + 2.0 * (v % 4.0))
            })
            .collect();
        std::fs::write(
            dir.path().join("many.csv"),
            format!("anid,Site,as,fe\n{}\n", rows.join("\n")),
        )
        .expect("write source");
        let commit = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/imports/commit")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ImportCommitRequest {
                            source: "many.csv".into(),
                            group_column: "Site".into(),
                            visible_id_column: None,
                            elemental_columns: None,
                            recipe: None,
                            destination_dir: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(commit.status(), axum::http::StatusCode::OK);
        let bytes = commit.into_body().collect().await.unwrap().to_bytes();
        let import: ImportCommitResponse = serde_json::from_slice(&bytes).unwrap();
        let path = import.groups[0].path.clone();

        let make_request = |path: String, columns: Vec<String>| {
            axum::http::Request::post("/api/v1/ordination/umap")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&UmapRequest {
                        path,
                        columns,
                        transformation: None,
                        seed: None,
                    })
                    .unwrap(),
                ))
                .unwrap()
        };

        let response = app
            .clone()
            .oneshot(make_request(path.clone(), vec!["as".into(), "fe".into()]))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let umap: UmapResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(umap.path, path);
        assert_eq!(umap.score_names, vec!["V1", "V2"]);
        assert_eq!(umap.embedding.len(), 20);
        assert!(umap.embedding.iter().all(|row| row.len() == 2));
        assert_eq!(umap.seed, 20260914);
        assert_eq!(umap.n_neighbors, 15);
        assert_eq!(umap.n_epochs, 200);
        assert!(umap.warnings.is_empty());

        // Same seed -> bit-identical embedding (class-D determinism).
        let response = app
            .clone()
            .oneshot(make_request(path.clone(), vec!["as".into(), "fe".into()]))
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let again: UmapResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(again.embedding, umap.embedding);

        // Fewer rows than n_neighbors -> 422 legacy gate.
        let small_path = commit_fixture(app.clone(), &dir).await.groups[0]
            .path
            .clone();
        let response = app
            .oneshot(make_request(small_path, vec!["as".into(), "fe".into()]))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.code, "validation_error");
    }

    #[tokio::test]
    async fn explore_views_round_trip_and_validation_error() {
        let (state, dir) = test_state();
        let app = root_router(state);
        let group_path = commit_fixture(app.clone(), &dir).await.groups[0]
            .path
            .clone();

        // Missing profile: both columns complete, column-order tie.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/explore/missing-profile")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExploreMissingProfileRequest {
                            path: group_path.clone(),
                            columns: vec!["as".into(), "fe".into()],
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let missing: ExploreMissingProfileResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(missing.rows.len(), 2);
        assert_eq!(missing.rows[0].feature, "as");
        assert_eq!(missing.rows[0].band, "Good");
        assert_eq!(missing.rows[1].num_missing, 0);

        // Histogram: bin counts over the finite values.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/explore/histogram")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExploreHistogramRequest {
                            path: group_path.clone(),
                            column: "as".into(),
                            bins: 2,
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let histogram: ExploreHistogramResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(histogram.column, "as");
        assert_eq!(histogram.breaks.len(), histogram.counts.len() + 1);
        assert_eq!(histogram.counts.iter().sum::<u64>(), 2);

        // Crosstab mean: one group level, legacy `result-<column>` name.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/explore/crosstab")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExploreCrosstabRequest {
                            path: group_path.clone(),
                            group_column: "Site".into(),
                            value_column: "as".into(),
                            summary_method: "mean".into(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let crosstab: ExploreCrosstabResponse = serde_json::from_slice(&bytes).unwrap();
        let CrosstabRows::Summary {
            result_column,
            rows,
        } = crosstab.rows
        else {
            panic!("summary kind");
        };
        assert_eq!(result_column, "result-as");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].group.as_deref(), Some("Baca"));
        assert!((rows[0].result.unwrap() - 1.75).abs() < 1e-12);

        // Compositional profile: row-major long table with group labels.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/explore/compositional-profile")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExploreCompositionalProfileRequest {
                            path: group_path.clone(),
                            columns: vec!["as".into(), "fe".into()],
                            group_column: Some("Site".into()),
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let profile: ExploreCompositionalProfileResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(profile.rows.len(), 4);
        assert_eq!(profile.rows[0].rowid, 1);
        assert_eq!(profile.rows[0].element, "as");
        assert_eq!(profile.rows[0].value, Some(1.5));
        assert_eq!(profile.rows[0].group_label.as_deref(), Some("Baca"));
        assert_eq!(profile.rows[1].element, "fe");

        // Unknown column: 422 validation error.
        let response = app
            .oneshot(
                axum::http::Request::post("/api/v1/explore/missing-profile")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExploreMissingProfileRequest {
                            path: group_path,
                            columns: vec!["zz".into()],
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.code, "validation_error");
    }

    #[tokio::test]
    async fn descriptive_patch_and_duplicate_round_trip_over_http() {
        let (state, dir) = test_state();
        let app = root_router(state);
        let commit = commit_fixture(app.clone(), &dir).await;
        let path = commit.groups[0].path.clone();

        // Hidden analytical UUID of the first row, read straight from the
        // committed group file.
        let before = archaeodash_data_io::read_group_file(dir.path().join(&path).as_path())
            .expect("read group");
        let uuid = before.rows[0].uuid.to_string();
        let revision = before.profile.revision_id.clone();
        drop(before);

        // PATCH descriptive-values edits one cell by UUID.
        let patch = PatchDescriptiveValuesRequest {
            path: path.clone(),
            expected_revision: revision,
            edits: vec![DescriptiveEdit {
                analytical_uuid: uuid,
                column: "Site".into(),
                value: Some("Zed".into()),
            }],
        };
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::patch("/api/v1/groups/descriptive-values")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&patch).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let tx: TransactionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(tx.action, "patch_descriptive_values");
        assert_eq!(tx.outputs[0].revision_id, "rev-2");

        // The elemental values are untouched; the descriptive cell changed.
        let after = archaeodash_data_io::read_group_file(dir.path().join(&path).as_path())
            .expect("read patched");
        assert_eq!(after.rows[0].descriptive[0].as_deref(), Some("Zed"));

        // Stale revision conflicts (409).
        let stale = PatchDescriptiveValuesRequest {
            path: path.clone(),
            expected_revision: "rev-1".into(),
            edits: vec![DescriptiveEdit {
                analytical_uuid: after.rows[1].uuid.to_string(),
                column: "Site".into(),
                value: None,
            }],
        };
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::patch("/api/v1/groups/descriptive-values")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&stale).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);

        // POST duplicate preserves UUIDs and lineage into a new group file.
        let duplicate = DuplicateGroupRequest {
            source_path: path.clone(),
            expected_revision: "rev-2".into(),
            new_group_name: "Baca Copy".into(),
            destination_path: None,
            preserve_uuids: true,
        };
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/groups/duplicate")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&duplicate).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let tx: TransactionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(tx.action, "duplicate_group");
        assert_eq!(tx.outputs[0].path, "groups/Baca_Copy.parquet");

        let copy = archaeodash_data_io::read_group_file(
            dir.path().join("groups/Baca_Copy.parquet").as_path(),
        )
        .expect("read duplicate");
        assert_eq!(copy.rows.len(), after.rows.len());
        let source_uuids: Vec<_> = after.rows.iter().map(|r| r.uuid).collect();
        let copy_uuids: Vec<_> = copy.rows.iter().map(|r| r.uuid).collect();
        assert_eq!(source_uuids, copy_uuids, "UUIDs preserved by default");
        assert_eq!(copy.profile.source_path, after.profile.source_path);

        // Elemental-column edits are rejected with the lock message (422).
        let locked = PatchDescriptiveValuesRequest {
            path,
            expected_revision: "rev-2".into(),
            edits: vec![DescriptiveEdit {
                analytical_uuid: copy.rows[0].uuid.to_string(),
                column: "as".into(),
                value: Some("9".into()),
            }],
        };
        let response = app
            .oneshot(
                axum::http::Request::patch("/api/v1/groups/descriptive-values")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&locked).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert!(envelope.message.contains("not a descriptive column"));
    }

    /// Section 7.3 export surface: measured-data, transformed, and PCA-score
    /// CSV exports are ephemeral JSON responses; a bad column is a 422.
    #[tokio::test]
    async fn export_routes_return_ephemeral_csv_and_validate() {
        use archaeodash_contracts::ExportMeasuredDataRequest;
        let (state, dir) = test_state();
        let app = root_router(state);
        let commit = commit_fixture(app.clone(), &dir).await;
        let path = commit.groups[0].path.clone();

        // Measured-data export: visible ID, descriptive, elemental columns;
        // the hidden analytical_uuid never appears in the CSV content.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/exports/measured-data")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExportMeasuredDataRequest {
                            path: path.clone(),
                            raw_text: false,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let export: ExportResult = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(export.media_type, "text/csv");
        assert_eq!(export.file_name, "Baca.csv");
        let lines: Vec<&str> = export.content.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines[0], "anid,Site,as,fe");
        assert_eq!(lines[1], "A1,Baca,1.5,3");
        assert!(!export.content.contains("analytical_uuid"));

        // Transformed export includes the ratio column, computed on demand.
        let definition = TransformationDefinition {
            name: "ratios".into(),
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
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/exports/transformed")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExportTransformedRequest {
                            path: path.clone(),
                            definition,
                            raw_text: false,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let export: ExportResult = serde_json::from_slice(&bytes).unwrap();
        assert!(export.content.contains("as_fe"));
        assert!(export.content.contains("0.5"));

        // PCA-score export follows the legacy pcadf shape (Section 3.2).
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::post("/api/v1/exports/pca-scores")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExportPcaScoresRequest {
                            path: path.clone(),
                            columns: vec!["as".into(), "fe".into()],
                            scale: false,
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let export: ExportResult = serde_json::from_slice(&bytes).unwrap();
        let lines: Vec<&str> = export.content.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines[0], "anid,Site,PC1,PC2");
        assert_eq!(lines.len(), commit.groups[0].row_count as usize + 1);

        // Unknown elemental column is a 422 validation error.
        let response = app
            .oneshot(
                axum::http::Request::post("/api/v1/exports/pca-scores")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ExportPcaScoresRequest {
                            path,
                            columns: vec!["cu".into()],
                            scale: false,
                            transformation: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.code, "validation_error");
    }

    /// `GET/PUT /api/v1/preferences`: typed allowlisted upsert, round trip,
    /// and a 422 on a shape-invalid value (Section 10.1).
    #[tokio::test]
    async fn preference_routes_upsert_validate_and_round_trip() {
        use archaeodash_contracts::{
            GetPreferencesResponse, PreferenceEntry, PreferenceKey, PutPreferenceRequest,
        };
        let (state, _dir) = test_state();
        let app = root_router(state);

        // Empty store reads as empty defaults.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/preferences")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<GetPreferencesResponse>(&bytes)
                .unwrap()
                .preferences,
            Vec::<PreferenceEntry>::new()
        );

        // Upsert theme, then lastOpenedDataset; get returns both.
        for (key, value) in [
            (PreferenceKey::Theme, serde_json::json!("dark")),
            (PreferenceKey::LastOpenedDataset, serde_json::json!("Baca")),
        ] {
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::put("/api/v1/preferences")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::to_vec(&PutPreferenceRequest { key, value }).unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
        }
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/v1/preferences")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let all = serde_json::from_slice::<GetPreferencesResponse>(&bytes).unwrap();
        assert_eq!(all.preferences.len(), 2);

        // Shape-invalid theme is a 422 validation_error.
        let response = app
            .oneshot(
                axum::http::Request::put("/api/v1/preferences")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&PutPreferenceRequest {
                            key: PreferenceKey::Theme,
                            value: serde_json::json!("solarized"),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}
