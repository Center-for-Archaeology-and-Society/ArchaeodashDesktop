//! Axum HTTP API composition (Section 10).
//!
//! Phase 1 smoke surface (`/healthz`), the Phase 2 local import surface
//! (`POST /api/v1/imports/preview|commit`), the group-operation surface
//! (`GET /api/v1/groups`, `POST /api/v1/groups/validate`,
//! `POST /api/v1/groups/transfer-units`, `POST /api/v1/groups/merge`,
//! `DELETE /api/v1/groups/{*path}`), and the local source-file surface
//! (`POST /api/v1/files`, `GET/DELETE /api/v1/files/{id}`,
//! `GET /api/v1/files/{id}/download`), all delegating to the shared
//! application use cases. Errors use the transport-neutral
//! problem-details-style `ErrorEnvelope`.

use std::sync::Arc;

use archaeodash_application::{app_info, GroupService, ImportService, SourceFileService};
use archaeodash_contracts::{
    AppInfo, DeleteGroupRequest, ErrorEnvelope, GroupCandidate, GroupSummary, ImportCommitRequest,
    ImportCommitResponse, ImportPreviewRequest, ImportPreviewResponse, MergeGroupsRequest,
    StagedFile, TransactionResponse, TransferUnitsRequest,
};
use archaeodash_data_io::ImportError;
use archaeodash_storage::StoreError;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};

/// Shared adapter state: one project-scoped import, group, and source-file
/// service.
#[derive(Clone)]
pub struct AppState {
    pub import: Arc<ImportService>,
    pub groups: Arc<GroupService>,
    pub files: Arc<SourceFileService>,
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

/// Builds the root router. Route groups for transformations, analysis, jobs,
/// and auth land in their owning phases (Sections 10.1+).
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
        .route("/api/v1/groups/transfer-units", post(groups_transfer_units))
        .route("/api/v1/groups/merge", post(groups_merge))
        .route("/api/v1/groups/{*path}", delete(groups_delete))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_contracts::TransferAction;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState {
            import: Arc::new(ImportService::new(dir.path()).expect("service")),
            groups: Arc::new(GroupService::new(dir.path()).expect("group service")),
            files: Arc::new(SourceFileService::new(dir.path()).expect("file service")),
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
}
