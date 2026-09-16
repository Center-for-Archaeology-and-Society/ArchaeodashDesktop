//! Axum HTTP API composition (Section 10).
//!
//! Phase 1 smoke surface (`/healthz`) plus the Phase 2 local import surface:
//! `POST /api/v1/imports/preview` and `POST /api/v1/imports/commit`, both
//! delegating to the shared `ImportService` use cases. Errors use the
//! transport-neutral problem-details-style `ErrorEnvelope`.

use std::sync::Arc;

use archaeodash_application::{app_info, ImportService};
use archaeodash_contracts::{
    AppInfo, ErrorEnvelope, ImportCommitRequest, ImportCommitResponse, ImportPreviewRequest,
    ImportPreviewResponse,
};
use archaeodash_data_io::ImportError;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

/// Shared adapter state: one project-scoped import service.
#[derive(Clone)]
pub struct AppState {
    pub import: Arc<ImportService>,
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

/// Builds the root router. Route groups for groups, transformations,
/// analysis, jobs, and auth land in their owning phases (Sections 10.1+).
pub fn root_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/imports/preview", post(imports_preview))
        .route("/api/v1/imports/commit", post(imports_commit))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState {
            import: Arc::new(ImportService::new(dir.path()).expect("service")),
        };
        (state, dir)
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
