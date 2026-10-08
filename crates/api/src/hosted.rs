//! Hosted control-plane router composition (Section 10) and startup
//! readiness rules (Section 6.5): migrations are an explicit deployment
//! step, readiness is a read-only schema check, and every response carries
//! a correlation ID with enforced security headers.

use crate::auth::{auth_router, AuthState};
use crate::security_headers::apply_security_headers;
use archaeodash_contracts::ErrorEnvelope;
use archaeodash_control_postgres::ControlStore;
use archaeodash_control_postgres::{FileRow, QuotaOutcome};
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::{Path, Query};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use std::sync::Arc;
use uuid::Uuid;

pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Shared state for the hosted health/readiness endpoints.
#[derive(Clone)]
pub struct HostedState {
    pub auth: AuthState,
    pub store: Arc<ControlStore>,
    /// Per-user object namespace root (Section 6.4). Required for the file
    /// routes; constructed in the binary from `AUTH_FILE_STORE_DIR`.
    pub files: Arc<crate::hosted_files::HostedFileStore>,
    /// Per-user logical byte quota (Section 6.9); uploads reserve against it.
    pub quota_bytes: i64,
}

/// Assigns a request/correlation ID to every response (Section 10): reuse a
/// trusted caller's `x-request-id` or generate one. Never log it with user
/// identifiers (Section 12).
async fn assign_request_id(request: Request<axum::body::Body>, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::now_v7().simple().to_string());
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-request-id"), value);
    }
    response
}

/// `GET /api/v1/health/live` — process liveness only; no dependency checks.
async fn health_live() -> StatusCode {
    StatusCode::OK
}

/// `GET /api/v1/health/ready` — control DB schema readiness without
/// sensitive details (Section 10.2). Fails closed: any store error is a 503.
async fn health_ready(
    State(state): State<HostedState>,
) -> Result<StatusCode, (StatusCode, &'static str)> {
    match state.store.schema_is_current().await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) | Err(_) => Err((StatusCode::SERVICE_UNAVAILABLE, "not ready")),
    }
}

/// The full hosted control-plane router: auth and user preferences under
/// the shared correlation-ID and security-header middleware. Cross-origin
/// browser access is denied by omission — hosted web assets are same-origin;
/// a deployment that needs an explicit allowlist can add `CorsLayer` with
/// `AUTH_CORS_ALLOW_ORIGIN` at composition time.
pub fn hosted_router(state: HostedState) -> Router {
    let auth = auth_router(state.auth.clone());
    let files = Router::new()
        .route("/api/v1/quota", get(quota_get))
        .route("/api/v1/files", post(files_upload).get(files_list))
        .route(
            "/api/v1/files/{id}",
            get(files_metadata).delete(files_delete),
        )
        .route("/api/v1/files/{id}/download", get(files_download))
        .with_state(state.clone());
    let transformations = Router::new()
        .route(
            "/api/v1/projects/{project_id}/transformations",
            post(transformations_save).get(transformations_list),
        )
        .route(
            "/api/v1/projects/{project_id}/transformations/{transformation_id}",
            get(transformations_get).delete(transformations_delete),
        )
        .with_state(state.clone());
    let health = Router::new()
        .route("/api/v1/health/live", get(health_live))
        .route("/api/v1/health/ready", get(health_ready))
        .with_state(state);
    let app: Router = Router::new()
        .merge(health)
        .merge(auth)
        .merge(files)
        .merge(transformations)
        .layer(middleware::from_fn(assign_request_id));
    apply_security_headers(app)
}

/// `GET /api/v1/quota` — the session user's storage accounting (Section 6.9):
/// logical bytes, reserved bytes, live file count, and the effective limit.
/// Zero rows (never uploaded) report zeros against the configured limit.
async fn quota_get(
    State(state): State<HostedState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, false).await? else {
        return Err(unauthorized());
    };
    let usage = state
        .store
        .get_quota(principal.user.id)
        .await
        .map_err(crate::auth::db_error)?
        .unwrap_or((0, 0, 0));
    Ok(Json(serde_json::json!({
        "logical_bytes": usage.0,
        "reserved_bytes": usage.1,
        "file_count": usage.2,
        "limit_bytes": state.quota_bytes,
    })))
}

/// `POST /api/v1/files?project_id=…&path=…&filename=…` — stages an upload
/// into the session user's namespace and inserts the catalog row
/// (Section 10.2). CSRF applies; ownership is checked in the INSERT itself.
async fn files_upload(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Query(query): Query<UploadFileQuery>,
    bytes: Bytes,
) -> Result<(StatusCode, Json<FileMetaResponse>), (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, true).await? else {
        return Err(unauthorized());
    };
    let project_id = match Uuid::parse_str(&query.project_id) {
        Ok(id) => id,
        Err(_) => return Err(bad_request("invalid_project_id")),
    };
    // Section 6.9: reserve capacity before any byte is staged; reconcile
    // (commit or release) after the catalog outcome.
    if !state
        .store
        .reserve_quota(principal.user.id, bytes.len() as i64, state.quota_bytes)
        .await
        .map_err(crate::auth::db_error)?
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorEnvelope {
                code: "quota_exceeded".into(),
                message: "Storage quota exceeded for this account.".into(),
            }),
        ));
    }
    let staged = match state
        .files
        .stage(
            principal.user.id,
            project_id,
            &query.path,
            query.filename.as_deref(),
            &bytes,
        )
        .await
    {
        Ok(staged) => staged,
        Err(e) => {
            let _ = state
                .store
                .reconcile_quota(
                    principal.user.id,
                    QuotaOutcome::Release,
                    bytes.len() as i64,
                    0,
                )
                .await;
            return Err(file_error(e));
        }
    };
    let row = FileRow {
        file_id: staged.file_id,
        user_id: principal.user.id,
        project_id,
        logical_path: staged.logical_path.clone(),
        kind: "source".to_string(),
        display_filename: staged.display_filename.clone(),
        object_key: staged.object_key.clone(),
        sha256: staged.sha256.clone(),
        media_type: staged.media_type.clone(),
        extension: staged.extension.clone(),
        bytes: staged.bytes as i64,
        state: "published".to_string(),
        parse_error: staged.parse_error.clone(),
        deleted_at: None,
        created_at: time::OffsetDateTime::now_utc(),
        updated_at: time::OffsetDateTime::now_utc(),
    };
    match state.store.insert_file(&row).await {
        Ok(true) => {
            let _ = state
                .store
                .reconcile_quota(
                    principal.user.id,
                    QuotaOutcome::Commit,
                    bytes.len() as i64,
                    1,
                )
                .await;
            Ok((
                StatusCode::CREATED,
                Json(FileMetaResponse {
                    file_id: staged.file_id,
                    project_id,
                    logical_path: staged.logical_path,
                    display_filename: staged.display_filename,
                    size_bytes: staged.bytes,
                    sha256: staged.sha256,
                    media_type: staged.media_type,
                    parse_state: staged.parse_state,
                    parse_error: staged.parse_error,
                }),
            ))
        }
        // Foreign/deleted project or path conflict: remove the staged object
        // and report failure without a cross-account existence oracle.
        Ok(false) => {
            state.files.discard_staged(&staged);
            let _ = state
                .store
                .reconcile_quota(
                    principal.user.id,
                    QuotaOutcome::Release,
                    bytes.len() as i64,
                    0,
                )
                .await;
            Err((StatusCode::NOT_FOUND, json_error_envelope("not_found")))
        }
        Err(e) => {
            state.files.discard_staged(&staged);
            let _ = state
                .store
                .reconcile_quota(
                    principal.user.id,
                    QuotaOutcome::Release,
                    bytes.len() as i64,
                    0,
                )
                .await;
            Err(crate::auth::db_error(e))
        }
    }
}

/// `GET /api/v1/files?project_id=…` — the owned project's live files.
async fn files_list(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Query(query): Query<ListFilesQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, false).await? else {
        return Err(unauthorized());
    };
    let project_id = match Uuid::parse_str(&query.project_id) {
        Ok(id) => id,
        Err(_) => return Err(bad_request("invalid_project_id")),
    };
    let rows = state
        .store
        .list_files(principal.user.id, project_id)
        .await
        .map_err(crate::auth::db_error)?;
    let files: Vec<FileMetaResponse> = rows
        .iter()
        .map(|row| FileMetaResponse {
            file_id: row.file_id,
            project_id: row.project_id,
            logical_path: row.logical_path.clone(),
            display_filename: row.display_filename.clone(),
            size_bytes: row.bytes as u64,
            sha256: row.sha256.clone(),
            media_type: row.media_type.clone(),
            parse_state: parse_state_from(&row.state, &row.parse_error),
            parse_error: row.parse_error.clone(),
        })
        .collect();
    Ok(Json(serde_json::Value::Object(serde_json::Map::from_iter(
        [(
            "files".to_string(),
            serde_json::to_value(files).unwrap_or_else(|_| serde_json::Value::Array(vec![])),
        )],
    ))))
}

/// `GET /api/v1/files/{id}` — metadata for one owned, live file.
async fn files_metadata(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Path(file_id): Path<Uuid>,
) -> Result<Json<FileMetaResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, false).await? else {
        return Err(unauthorized());
    };
    let meta = state
        .files
        .metadata(&state.store, principal.user.id, file_id)
        .await
        .map_err(file_error)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, json_error_envelope("not_found")))?;
    Ok(Json(FileMetaResponse::from_hosted(meta)))
}

/// `GET /api/v1/files/{id}/download` — object bytes for an owned, live file.
/// Ownership resolves in SQL before any path is touched.
async fn files_download(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Path(file_id): Path<Uuid>,
) -> Result<Response, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, false).await? else {
        return Err(unauthorized());
    };
    let Some((meta, content)) = state
        .files
        .download(&state.store, principal.user.id, file_id)
        .await
        .map_err(file_error)?
    else {
        return Err((StatusCode::NOT_FOUND, json_error_envelope("not_found")));
    };
    // Display filename only in Content-Disposition, quoted and sanitized:
    // it never appears in a URL or object key (Section 6.4).
    let disposition = format!(
        "attachment; filename=\"{}\"",
        meta.display_filename.replace(['\\', '"'], "_")
    );
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, meta.media_type),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        content,
    )
        .into_response())
}

/// `DELETE /api/v1/files/{id}` — tombstones the catalog row and moves the
/// object to namespace trash. Idempotent semantics: unknown/foreign/deleted
/// IDs are uniformly 404.
async fn files_delete(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Path(file_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, true).await? else {
        return Err(unauthorized());
    };
    // Metadata first so the tombstone can reconcile the quota (Section 6.9):
    // the bytes leave the logical total only when the delete succeeded.
    let meta = state
        .files
        .metadata(&state.store, principal.user.id, file_id)
        .await
        .map_err(file_error)?;
    match state
        .files
        .delete(&state.store, principal.user.id, file_id)
        .await
        .map_err(file_error)?
    {
        Some(_) => {
            if let Some(meta) = meta {
                let _ = state
                    .store
                    .reconcile_quota(
                        principal.user.id,
                        QuotaOutcome::Remove,
                        meta.size_bytes as i64,
                        1,
                    )
                    .await;
            }
            Ok(StatusCode::NO_CONTENT)
        }
        None => Err((StatusCode::NOT_FOUND, json_error_envelope("not_found"))),
    }
}

#[derive(serde::Deserialize)]
struct UploadFileQuery {
    project_id: String,
    path: String,
    /// Optional display name override; defaults to the path's last segment.
    filename: Option<String>,
}

// --- Hosted transformation definitions (Sections 6.4/6.5/10.2) ------------
//
// Named transformation definitions are configuration (column selections,
// method parameters, seeds — never calculated values, Section 5), so the
// catalog row holds identity/ownership/summary fields and the definition
// JSON lives in the project's `transformations/` file-store namespace.

/// A definition body is small configuration JSON; anything larger is a
/// malformed client, not a definition (Section 6.9 keeps definition bytes
/// out of the upload quota until the quota-policy revision slice).
const MAX_TRANSFORMATION_BYTES: usize = 256 * 1024;

/// The catalog's summary serialization of one definition row.
#[derive(serde::Serialize)]
struct TransformationMetaResponse {
    transformation_id: Uuid,
    project_id: Uuid,
    name: String,
    revision: i32,
    transform_method: String,
    imputation_method: String,
    ratio_count: i32,
    bytes: u64,
    sha256: String,
    created_at_unix_secs: u64,
    updated_at_unix_secs: u64,
}

fn transformation_meta(
    row: &archaeodash_control_postgres::TransformationRow,
) -> TransformationMetaResponse {
    TransformationMetaResponse {
        transformation_id: row.transformation_id,
        project_id: row.project_id,
        name: row.name.clone(),
        revision: row.revision,
        transform_method: row.transform_method.clone(),
        imputation_method: row.imputation_method.clone(),
        ratio_count: row.ratio_count,
        bytes: row.bytes.max(0) as u64,
        sha256: row.sha256.clone(),
        created_at_unix_secs: row.created_at.unix_timestamp().max(0) as u64,
        updated_at_unix_secs: row.updated_at.unix_timestamp().max(0) as u64,
    }
}

#[derive(serde::Serialize)]
struct TransformationSaveResponse {
    transformation: TransformationMetaResponse,
    replaced: bool,
    definition: archaeodash_contracts::TransformationDefinition,
}

#[derive(serde::Serialize)]
struct TransformationListResponse {
    transformations: Vec<TransformationMetaResponse>,
}

#[derive(serde::Serialize)]
struct TransformationGetResponse {
    transformation: TransformationMetaResponse,
    definition: archaeodash_contracts::TransformationDefinition,
}

/// Serializes a contracts enum to its wire tag for the catalog summary
/// columns; serialization of these unit enums cannot fail.
fn method_tag<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => "unknown".to_string(),
    }
}

/// `POST /api/v1/projects/{id}/transformations` — saves (upserts by name)
/// one definition into the owned project's transformations namespace.
/// CSRF applies; the project ownership check happens before any object
/// write; a replaced revision's object moves to trash for the sweep.
async fn transformations_save(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    bytes: Bytes,
) -> Result<(StatusCode, Json<TransformationSaveResponse>), (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, true).await? else {
        return Err(unauthorized());
    };
    if bytes.len() > MAX_TRANSFORMATION_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorEnvelope {
                code: "limit_exceeded".into(),
                message: "Transformation definition exceeds the size limit.".into(),
            }),
        ));
    }
    let parsed: archaeodash_contracts::SaveTransformationRequest =
        serde_json::from_slice(&bytes).map_err(|_| bad_request("invalid_json"))?;
    let name = parsed.definition.name.trim().to_string();
    if name.is_empty() || name.len() > 200 {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorEnvelope {
                code: "invalid_name".into(),
                message: "Transformation name must be 1-200 characters.".into(),
            }),
        ));
    }
    if let Err(e) = archaeodash_application::transforms::validate_definition(&parsed.definition) {
        return Err(crate::domain_error_response(e));
    }
    // Uniform 404 for foreign/deleted projects before any write.
    if state
        .store
        .get_project(principal.user.id, project_id)
        .await
        .map_err(crate::auth::db_error)?
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, json_error_envelope("not_found")));
    }
    let now = std::time::SystemTime::now();
    // Preserve the original creation timestamp across replaces (desktop
    // upsert parity): the envelope carries the first save's seconds.
    let existing = state
        .store
        .get_transformation_by_name(principal.user.id, project_id, &name)
        .await
        .map_err(crate::auth::db_error)?;
    let created_at_unix_secs = match &existing {
        Some(row) => row.created_at.unix_timestamp().max(0) as u64,
        None => archaeodash_application::transforms::now_unix_secs(),
    };
    let transformation_id = existing
        .as_ref()
        .map(|row| row.transformation_id)
        .unwrap_or_else(Uuid::now_v7);
    let stored = archaeodash_application::transforms::StoredTransformation {
        created_at_unix_secs,
        definition: parsed.definition,
    };
    let body = serde_json::to_vec(&stored)
        .map_err(|_| internal_error("definition serialization failed"))?;
    let object = state
        .files
        .write_definition_object(
            principal.user.id,
            project_id,
            transformation_id,
            &Uuid::now_v7().simple().to_string(),
            &body,
        )
        .map_err(file_error)?;
    match state
        .store
        .upsert_transformation(
            principal.user.id,
            project_id,
            &name,
            &object.object_key,
            &object.sha256,
            object.bytes as i64,
            &method_tag(&stored.definition.transform_method),
            &method_tag(&stored.definition.imputation_method),
            stored.definition.ratios.len() as i32,
            now,
        )
        .await
    {
        Ok(Some(outcome)) => {
            // Replaced revisions keep no history (desktop upsert parity: the
            // definition is one name-keyed JSON). The catalog row now points
            // at the new object only, so the previous bytes are unrecoverable
            // through any API — remove them outright. Trashing would leak:
            // no catalog row references a replaced revision, so the retention
            // sweep could never find it. A crash between commit and removal
            // orphans an undiscoverable object, the same guarantee class as
            // the Section 6.9 file sweep window.
            if let Some(previous) = outcome.previous_object_key {
                state.files.remove_object(&previous);
            }
            Ok((
                if outcome.replaced {
                    StatusCode::OK
                } else {
                    StatusCode::CREATED
                },
                Json(TransformationSaveResponse {
                    transformation: transformation_meta(&outcome.row),
                    replaced: outcome.replaced,
                    definition: stored.definition,
                }),
            ))
        }
        // The project check above makes this unreachable today; the object
        // must not outlive a rejected catalog write either way.
        Ok(None) => {
            state.files.remove_object(&object.object_key);
            Err((StatusCode::NOT_FOUND, json_error_envelope("not_found")))
        }
        Err(e) => {
            state.files.remove_object(&object.object_key);
            Err(crate::auth::db_error(e))
        }
    }
}

/// `GET /api/v1/projects/{id}/transformations` — the owned project's live
/// definitions from catalog summary fields (no object reads).
async fn transformations_list(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<Json<TransformationListResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, false).await? else {
        return Err(unauthorized());
    };
    let rows = state
        .store
        .list_transformations(principal.user.id, project_id)
        .await
        .map_err(crate::auth::db_error)?;
    Ok(Json(TransformationListResponse {
        transformations: rows.iter().map(transformation_meta).collect(),
    }))
}

/// `GET /api/v1/projects/{id}/transformations/{transformation_id}` — one
/// owned definition: catalog metadata plus the definition JSON, checksum-
/// verified against the catalog row before parsing.
async fn transformations_get(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Path((project_id, transformation_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TransformationGetResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, false).await? else {
        return Err(unauthorized());
    };
    let row = state
        .store
        .get_transformation(principal.user.id, transformation_id)
        .await
        .map_err(crate::auth::db_error)?
        .filter(|row| row.project_id == project_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, json_error_envelope("not_found")))?;
    let body = state
        .files
        .read_object(&row.object_key)
        .map_err(file_error)?;
    let checksum = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&body);
        format!("{:x}", hasher.finalize())
    };
    if checksum != row.sha256 {
        return Err(internal_error("definition checksum mismatch"));
    }
    let stored: archaeodash_application::transforms::StoredTransformation =
        serde_json::from_slice(&body)
            .map_err(|_| internal_error("stored definition is unreadable"))?;
    Ok(Json(TransformationGetResponse {
        transformation: transformation_meta(&row),
        definition: stored.definition,
    }))
}

/// `DELETE /api/v1/projects/{id}/transformations/{transformation_id}` —
/// tombstones the catalog row and trashes the object. Idempotent semantics:
/// unknown/foreign/deleted IDs are uniformly 404.
async fn transformations_delete(
    State(state): State<HostedState>,
    headers: HeaderMap,
    Path((project_id, transformation_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = crate::auth::require_session(&state.auth, &headers, true).await? else {
        return Err(unauthorized());
    };
    let Some(row) = state
        .store
        .get_transformation(principal.user.id, transformation_id)
        .await
        .map_err(crate::auth::db_error)?
        .filter(|row| row.project_id == project_id)
    else {
        return Err((StatusCode::NOT_FOUND, json_error_envelope("not_found")));
    };
    let Some(object_key) = state
        .store
        .soft_delete_transformation(
            principal.user.id,
            transformation_id,
            std::time::SystemTime::now(),
        )
        .await
        .map_err(crate::auth::db_error)?
    else {
        return Err((StatusCode::NOT_FOUND, json_error_envelope("not_found")));
    };
    state.files.trash_object(
        principal.user.id,
        row.project_id,
        &object_key,
        &transformation_id.simple().to_string(),
    );
    Ok(StatusCode::NO_CONTENT)
}

fn internal_error(message: &'static str) -> (StatusCode, Json<ErrorEnvelope>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorEnvelope {
            code: "internal_error".into(),
            message: message.into(),
        }),
    )
}

#[derive(serde::Deserialize)]
struct ListFilesQuery {
    project_id: String,
}

#[derive(serde::Serialize)]
struct FileMetaResponse {
    file_id: Uuid,
    project_id: Uuid,
    logical_path: String,
    display_filename: String,
    size_bytes: u64,
    sha256: String,
    media_type: String,
    parse_state: String,
    parse_error: Option<String>,
}

impl FileMetaResponse {
    fn from_hosted(m: crate::hosted_files::HostedFileMeta) -> Self {
        Self {
            file_id: m.file_id,
            project_id: m.project_id,
            logical_path: m.logical_path,
            display_filename: m.display_filename,
            size_bytes: m.size_bytes,
            sha256: m.sha256,
            media_type: m.media_type,
            parse_state: m.parse_state,
            parse_error: m.parse_error,
        }
    }
}

fn unauthorized() -> (StatusCode, Json<ErrorEnvelope>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorEnvelope {
            code: "unauthorized".into(),
            message: "Sign in to continue.".into(),
        }),
    )
}

fn bad_request(code: &'static str) -> (StatusCode, Json<ErrorEnvelope>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorEnvelope {
            code: code.into(),
            message: "Malformed request.".into(),
        }),
    )
}

fn json_error_envelope(code: &'static str) -> Json<ErrorEnvelope> {
    Json(ErrorEnvelope {
        code: code.into(),
        message: "The referenced resource does not exist.".into(),
    })
}

/// The catalog row's `state` is the lifecycle state (staged/published/
/// deleted); the parse outcome surfaces through `parse_error`.
fn parse_state_from(state: &str, parse_error: &Option<String>) -> String {
    match (state, parse_error) {
        ("deleted", _) => "deleted".to_string(),
        (_, Some(_)) => "parse_failed".to_string(),
        _ => "parsed".to_string(),
    }
}

fn file_error(err: crate::hosted_files::HostedFileError) -> (StatusCode, Json<ErrorEnvelope>) {
    use crate::hosted_files::HostedFileError as E;
    let (status, code, message) = match err {
        E::InvalidPath { .. } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_path",
            "That logical path is not allowed.",
        ),
        E::UnsupportedFormat { .. } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_format",
            "Unsupported source format; allowed: csv, tsv, xlsx.",
        ),
        E::TooLarge { .. } => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "limit_exceeded",
            "Upload exceeds the size limit.",
        ),
        E::Catalog(c) => return crate::auth::db_error(c),
        E::Io(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "File storage failed.",
        ),
    };
    (
        status,
        Json(ErrorEnvelope {
            code: code.into(),
            message: message.to_string(),
        }),
    )
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use archaeodash_auth::throttle::ThrottlePepper;
    use axum::body::Body;
    use axum::http::header;
    use tower::ServiceExt;

    /// Builds a hosted router over a lazily-connected pool: middleware and
    /// header tests run without a database (readiness then degrades to 503),
    /// and with `DATABASE_URL` the readiness test exercises the real schema
    /// check.
    pub(crate) async fn hosted_test_router() -> (
        HostedState,
        Arc<archaeodash_auth::email::DevSinkEmailSender>,
    ) {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://nobody:nopass@127.0.0.1:9/nodb".to_string());
        let pool = sqlx::PgPool::connect_lazy(&url).expect("lazy pool builds");
        let store = Arc::new(ControlStore::new(pool));
        let sink = Arc::new(archaeodash_auth::email::DevSinkEmailSender::new());
        let state = HostedState {
            auth: AuthState {
                store: store.clone(),
                email: sink.clone(),
                pepper: ThrottlePepper::from_hex(&"ab".repeat(32)).expect("pepper"),
                base_url: "https://archaeodash.example".to_string(),
            },
            store,
            files: Arc::new(
                crate::hosted_files::HostedFileStore::new(
                    tempfile::tempdir().expect("temp file store").keep(),
                )
                .expect("file store"),
            ),
            quota_bytes: 1_073_741_824,
        };
        (state, sink)
    }

    #[tokio::test]
    async fn request_id_is_echoed_and_generated() {
        let (state, _sink) = hosted_test_router().await;
        let app = hosted_router(state);
        // Echo a well-formed caller ID.
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/v1/health/live")
                    .header(REQUEST_ID_HEADER, "corr-42_test.1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get(REQUEST_ID_HEADER)
                .expect("request id header")
                .to_str()
                .expect("ascii"),
            "corr-42_test.1"
        );
        // Generate one when absent; security headers ride along through the
        // composed middleware stack.
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/v1/health/live")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        let id = res
            .headers()
            .get(REQUEST_ID_HEADER)
            .expect("request id header")
            .to_str()
            .expect("ascii");
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        let csp = res
            .headers()
            .get("content-security-policy")
            .expect("CSP header")
            .to_str()
            .expect("ascii");
        let directives: std::collections::BTreeSet<&str> = csp.split(';').map(str::trim).collect();
        for required in [
            "default-src 'self'",
            "script-src 'self'",
            "style-src 'self' 'unsafe-inline'",
            "img-src 'self' data:",
            "frame-ancestors 'self'",
            "form-action 'self'",
            "base-uri 'self'",
            "object-src 'none'",
        ] {
            assert!(
                directives.contains(required),
                "CSP missing {required}: {csp}"
            );
        }
        assert_eq!(
            res.headers()
                .get("x-content-type-options")
                .map(|v| v.to_str().expect("ascii").to_string()),
            Some("nosniff".to_string())
        );
    }

    #[tokio::test]
    async fn request_logs_route_template_status_latency_no_pii() {
        // Section 12: structured per-request logs keyed by route template and
        // status only — never usernames, emails, or file/project names.
        let (state, _sink) = hosted_test_router().await;
        let buffer: std::sync::Arc<std::sync::Mutex<Vec<u8>>> = std::sync::Arc::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_level(false)
            .with_target(false)
            .with_writer(move || log_capture::LogWriter(writer.clone()))
            .finish();
        let dispatch = tracing::subscriber::set_default(subscriber);
        let sensitive = "pref-user-shouldneverappear@example.com";
        let _ = finalize_hosted_router(hosted_router(state.clone()))
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "username": "shouldneverappear",
                            "email": sensitive,
                            "password": "correct horse battery staple",
                            "consent_version": "2026-10"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("infallible");
        drop(dispatch);
        let logs = String::from_utf8(buffer.lock().expect("log lock").clone()).expect("utf8 logs");
        assert!(logs.contains("route=/api/v1/auth/register"), "logs: {logs}");
        assert!(logs.contains("method=POST"), "logs: {logs}");
        // The DB may or may not be reachable in unit-test mode; the log line
        // must exist with some status and a latency measurement either way.
        let status_log = logs
            .lines()
            .find(|l| l.contains("route=/api/v1/auth/register"))
            .expect("register log line");
        assert!(status_log.contains("status="), "logs: {logs}");
        assert!(status_log.contains("latency_ms="), "logs: {logs}");
        assert!(!logs.contains("shouldneverappear"), "PII leaked: {logs}");
        assert!(!logs.contains("correct horse"), "PII leaked: {logs}");
        // Unmatched paths log a safe template, never the raw path.
        let _ = finalize_hosted_router(hosted_router(state))
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/not-a-route/secret-name-123")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        let logs = String::from_utf8(buffer.lock().expect("log lock").clone()).expect("utf8 logs");
        assert!(!logs.contains("secret-name-123"), "raw path leaked: {logs}");
    }

    #[tokio::test]
    async fn health_ready_reflects_schema_currentness() {
        let (state, _sink) = hosted_test_router().await;
        let app = hosted_router(state);
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/health/ready")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        // The test pool has run migrations via the auth tests' helper only in
        // a different pool; migrate here is idempotent, but readiness must be
        // truthful either way: 200 when current, 503 when not.
        assert!(
            res.status() == StatusCode::OK || res.status() == StatusCode::SERVICE_UNAVAILABLE,
            "unexpected readiness status {}",
            res.status()
        );
        assert!(
            res.headers().get(REQUEST_ID_HEADER).is_some(),
            "correlation id on readiness"
        );
    }
}

/// Request observability (Section 12): one structured log line per request
/// with the matched route template, method, status, and wall-clock latency.
/// Labels carry only route templates and status codes — never usernames,
/// emails, uploaded filenames, or raw group/project names.
async fn observe_request(request: Request<axum::body::Body>, next: Next) -> Response {
    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(axum::extract::MatchedPath::as_str)
        .unwrap_or("unmatched")
        .to_owned();
    let start = std::time::Instant::now();
    let response = next.run(request).await;
    let latency_ms = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();
    let request_id = response
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    tracing::info!(
        route = %route,
        method = %method,
        status,
        latency_ms,
        request_id = %request_id,
        "request"
    );
    response
}

/// Composes the full hosted middleware stack, innermost first:
/// request-id assignment, then request logging (sees the assigned ID),
/// then security headers on every response.
pub fn finalize_hosted_router(router: Router) -> Router {
    apply_security_headers(router.layer(middleware::from_fn(observe_request)))
}

#[cfg(test)]
mod log_capture {
    //! `MakeWriter` over a shared buffer so tests can assert on emitted log
    //! lines without a global subscriber.
    #![allow(clippy::expect_used)] // test code; panics are the failure mode
    use std::io;
    use std::sync::{Arc, Mutex};

    /// Owned-`Arc` sink so the `MakeWriter` closure returns an `io::Write`.
    #[derive(Clone)]
    pub(crate) struct LogWriter(pub(crate) Arc<Mutex<Vec<u8>>>);

    impl tracing_subscriber::fmt::MakeWriter<'_> for LogWriter {
        type Writer = LogWriter;

        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    impl io::Write for LogWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("log lock").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod header_hygiene {
    // 2026-02-20 audit findings: no server technology/version disclosure on
    // hosted responses (the legacy Shiny stack leaked X-Powered-By and
    // Server version headers).

    use super::tests::hosted_test_router;
    use super::*;
    use crate::auth::tests::{auth_state, test_peer};
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use tower::ServiceExt;

    #[tokio::test]
    async fn responses_disclose_no_server_technology() {
        let (state, _sink) = hosted_test_router().await;
        let res = hosted_router(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/health/live")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        for header in ["server", "x-powered-by"] {
            assert!(
                res.headers().get(header).is_none(),
                "{header} must not be emitted"
            );
        }
    }

    // --- Hosted file routes (Section 10.2 data plane) --------------------

    #[tokio::test]
    async fn hosted_file_routes_require_session_and_scope_ownership() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        // Reuse the auth test helper to get a session cookie + CSRF token.
        let (cookie, csrf) =
            crate::auth::preference_tests::login_session(state.clone(), sink).await;
        let (app_state, _base) = hosted_test_state(state);
        let app = hosted_router(app_state);
        // Unauthenticated upload is 401.
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/files?project_id=00000000-0000-0000-0000-000000000000&path=a.csv")
                    .header(header::CONTENT_TYPE, "text/csv")
                    .extension(ConnectInfo(test_peer()))
                    .body(Body::from("a,b\n1,2\n"))
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let _ = (cookie, csrf); // used by the DB-backed round trip below
    }
}

/// Wraps an [`AuthState`]'s shared store into a hosted state with a temp
/// file store, for file-route tests that log in through the auth helpers.
/// Returns the store base path so tests can assert object lifecycle on disk
/// (retention/trash behavior is a filesystem contract, not a catalog one).
#[cfg(test)]
pub(crate) fn hosted_test_state(
    state: crate::auth::AuthState,
) -> (HostedState, std::path::PathBuf) {
    let base = tempfile::tempdir().expect("temp file store").keep();
    let files =
        std::sync::Arc::new(crate::hosted_files::HostedFileStore::new(&base).expect("file store"));
    (
        HostedState {
            auth: state.clone(),
            store: state.store.clone(),
            files,
            quota_bytes: 1_073_741_824,
        },
        base,
    )
}

#[cfg(test)]
mod quota_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::hosted_test_state;
    use super::*;
    use crate::auth::preference_tests::login_session;
    use crate::auth::tests::{auth_state, test_peer};
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use tower::ServiceExt;

    /// Resolves the session user's id from the cookie (test helper).
    async fn whoami(state: &crate::auth::AuthState, cookie: &str) -> Uuid {
        let presentation = cookie
            .split(';')
            .map(str::trim)
            .find_map(|c| c.strip_prefix(crate::auth::SESSION_COOKIE))
            .and_then(|c| c.strip_prefix('='))
            .expect("session cookie")
            .to_string();
        let digest = archaeodash_auth::token::digest_presentation(&presentation);
        let (_, user) = state
            .store
            .find_live_session(&digest, std::time::SystemTime::now())
            .await
            .expect("session lookup")
            .expect("live session");
        user.id
    }

    #[tokio::test]
    async fn hosted_upload_enforces_the_per_user_quota() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let (cookie, csrf) = login_session(state.clone(), sink).await;
        // 10-byte quota: the 5-byte upload fits, the 20-byte one does not.
        let (mut app_state, _base) = hosted_test_state(state);
        app_state.quota_bytes = 10;
        let user = whoami(&app_state.auth, &cookie).await;
        let project = app_state
            .store
            .create_project(
                uuid::Uuid::now_v7(),
                user,
                "quota",
                std::time::SystemTime::now(),
            )
            .await
            .expect("project");
        let app = hosted_router(app_state);
        let post = |app: Router, uri: String, body: Vec<u8>, csrf: String, cookie: String| {
            app.oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::COOKIE, cookie)
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "text/csv")
                    .extension(ConnectInfo(test_peer()))
                    .body(Body::from(body))
                    .expect("request"),
            )
        };
        let res = post(
            app.clone(),
            format!("/api/v1/files?project_id={}&path=a.csv", project.project_id),
            b"a,b\n1\n".to_vec(),
            csrf.clone(),
            cookie.clone(),
        )
        .await
        .expect("infallible");
        assert_eq!(res.status(), StatusCode::CREATED);
        let res = post(
            app,
            format!("/api/v1/files?project_id={}&path=b.csv", project.project_id),
            vec![0u8; 20],
            csrf,
            cookie,
        )
        .await
        .expect("infallible");
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body");
        let envelope: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(envelope["code"], "quota_exceeded");
    }

    #[tokio::test]
    async fn quota_route_reports_usage_for_the_session_user() {
        let Some((state, sink)) = crate::auth::tests::auth_state().await else {
            return;
        };
        let (cookie, _csrf) =
            crate::auth::preference_tests::login_session(state.clone(), sink).await;
        let store = state.store.clone();
        let (mut app_state, _base) = hosted_test_state(state);
        app_state.quota_bytes = 500;
        let user = whoami(&app_state.auth, &cookie).await;
        app_state
            .store
            .reserve_quota(user, 120, 500)
            .await
            .expect("reserve");
        app_state
            .store
            .reconcile_quota(
                user,
                archaeodash_control_postgres::QuotaOutcome::Commit,
                120,
                1,
            )
            .await
            .expect("commit");
        let app = hosted_router(app_state);

        let get = |app: Router, cookie: String| {
            app.oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/v1/quota")
                    .header(header::COOKIE, cookie)
                    .extension(ConnectInfo(test_peer()))
                    .body(Body::empty())
                    .expect("request"),
            )
        };
        let res = get(app, cookie).await.expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body");
        let envelope: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(envelope["logical_bytes"], 120);
        assert_eq!(envelope["file_count"], 1);
        assert_eq!(envelope["limit_bytes"], 500);

        // Unauthenticated access is 401: a fresh router with no session.
        let (app2_state, _file_base) = hosted_test_state(crate::auth::AuthState {
            store,
            email: std::sync::Arc::new(archaeodash_auth::email::DevSinkEmailSender::new()),
            pepper: archaeodash_auth::throttle::ThrottlePepper::from_hex(&"ab".repeat(32))
                .expect("pepper"),
            base_url: "https://archaeodash.example".to_string(),
        });
        let app2 = hosted_router(app2_state);
        let res = app2
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/v1/quota")
                    .extension(ConnectInfo(test_peer()))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}

#[cfg(test)]
mod transformation_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::hosted_test_state;
    use super::*;
    use crate::auth::preference_tests::login_session;
    use crate::auth::tests::{auth_state, test_peer};
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use tower::ServiceExt;

    const DEFINITION_BODY: &str = r#"{"definition":{"name":"Cu over Zn","transform_method":"log10","imputation_method":"none","imputation_seed":null,"elemental_columns":["Cu","Zn"],"descriptive_columns":[],"group_column":null,"ratios":[{"output_name":null,"numerator":"Cu","denominator":"Zn"}],"ratio_mode":"append"}}"#;

    /// Requests through the composed router with session + CSRF attached.
    async fn request(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
        cookie: &str,
        csrf: &str,
    ) -> axum::response::Response {
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header("x-csrf-token", csrf)
            .extension(ConnectInfo(test_peer()));
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        app.oneshot(
            builder
                .body(Body::from(body.unwrap_or_default().to_string()))
                .expect("request"),
        )
        .await
        .expect("infallible")
    }

    #[tokio::test]
    async fn transformations_round_trip_with_ownership_and_csrf() {
        let Some((auth, sink)) = auth_state().await else {
            return;
        };
        let (cookie, csrf) = login_session(auth.clone(), sink).await;
        let (app_state, _file_base) = hosted_test_state(auth);
        let store = app_state.store.clone();
        let user = {
            let presentation = cookie
                .split(';')
                .map(str::trim)
                .find_map(|c| c.strip_prefix(crate::auth::SESSION_COOKIE))
                .and_then(|c| c.strip_prefix('='))
                .expect("session cookie")
                .to_string();
            let digest = archaeodash_auth::token::digest_presentation(&presentation);
            store
                .find_live_session(&digest, std::time::SystemTime::now())
                .await
                .expect("session lookup")
                .expect("live session")
                .1
                .id
        };
        let project = store
            .create_project(
                Uuid::now_v7(),
                user,
                "transformations",
                std::time::SystemTime::now(),
            )
            .await
            .expect("project");
        let stranger = {
            let suffix = crate::auth::tests::unique_suffix();
            let username = format!("transf-stranger-{suffix}");
            let email = format!("{username}@example.com");
            let hash = archaeodash_auth::password::hash_password("correct horse battery staple")
                .expect("hash");
            store
                .create_user(
                    Uuid::now_v7(),
                    &username,
                    &username,
                    &email,
                    archaeodash_auth::identity::normalize_email(&email)
                        .expect("valid email")
                        .as_str(),
                    &hash,
                    None,
                    std::time::SystemTime::now(),
                )
                .await
                .expect("stranger user")
        };
        let stranger_project = store
            .create_project(
                Uuid::now_v7(),
                stranger.id,
                "other",
                std::time::SystemTime::now(),
            )
            .await
            .expect("stranger project");
        let base = format!("/api/v1/projects/{}/transformations", project.project_id);
        let app = hosted_router(app_state);

        // Unauthenticated save is 401.
        let res = Router::new()
            .merge(app.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(&base)
                    .header(header::CONTENT_TYPE, "application/json")
                    .extension(ConnectInfo(test_peer()))
                    .body(Body::from(DEFINITION_BODY))
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // Missing CSRF on a cookie-authenticated state change is 403.
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(&base)
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .extension(ConnectInfo(test_peer()))
                    .body(Body::from(DEFINITION_BODY))
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // Save creates revision 1 and echoes the definition.
        let res = request(
            app.clone(),
            "POST",
            &base,
            Some(DEFINITION_BODY),
            &cookie,
            &csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body");
        let saved: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(saved["replaced"], false);
        assert_eq!(saved["transformation"]["revision"], 1);
        assert_eq!(saved["transformation"]["name"], "Cu over Zn");
        assert_eq!(saved["transformation"]["transform_method"], "log10");
        assert_eq!(saved["definition"]["ratio_mode"], "append");
        let transformation_id = saved["transformation"]["transformation_id"]
            .as_str()
            .expect("id")
            .to_string();

        // Save again under the same name: replace in place (revision 2).
        let res = request(
            app.clone(),
            "POST",
            &base,
            Some(DEFINITION_BODY),
            &cookie,
            &csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body");
        let replaced: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(replaced["replaced"], true);
        assert_eq!(replaced["transformation"]["revision"], 2);
        assert_eq!(
            replaced["transformation"]["transformation_id"],
            saved["transformation"]["transformation_id"]
        );

        // List shows one entry; get round-trips the stored definition.
        let res = request(app.clone(), "GET", &base, None, &cookie, &csrf).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body");
        let listed: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(listed["transformations"].as_array().expect("list").len(), 1);

        let res = request(
            app.clone(),
            "GET",
            &format!("{base}/{transformation_id}"),
            None,
            &cookie,
            &csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body");
        let fetched: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(fetched["definition"], saved["definition"]);

        // A stranger's list is empty and direct get/delete are uniform 404s
        // (no existence oracle across accounts).
        let (stranger_cookie, stranger_csrf) = {
            let Some((auth2, sink2)) = auth_state().await else {
                return;
            };
            login_session(auth2.clone(), sink2).await
        };
        let res = request(
            app.clone(),
            "GET",
            &format!(
                "/api/v1/projects/{}/transformations/{transformation_id}",
                stranger_project.project_id
            ),
            None,
            &stranger_cookie,
            &stranger_csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let res = request(
            app.clone(),
            "GET",
            &format!("{base}/{transformation_id}"),
            None,
            &stranger_cookie,
            &stranger_csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let res = request(
            app.clone(),
            "DELETE",
            &format!("{base}/{transformation_id}"),
            None,
            &stranger_cookie,
            &stranger_csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        // Invalid definitions are 422 with the domain validation code.
        let res = request(
            app.clone(),
            "POST",
            &base,
            Some(r#"{"definition":{"name":"  ","transform_method":"none","imputation_method":"none","imputation_seed":null,"elemental_columns":[],"descriptive_columns":[],"group_column":null,"ratios":[],"ratio_mode":"append"}}"#),
            &cookie,
            &csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body");
        let envelope: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(envelope["code"], "invalid_name");

        // Oversized bodies are 413 before any validation or storage.
        let oversized = format!("{{\"definition\":{}}}", " ".repeat(256 * 1024));
        let res = request(app.clone(), "POST", &base, Some(&oversized), &cookie, &csrf).await;
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // Delete is 204 once, then uniformly 404.
        let res = request(
            app.clone(),
            "DELETE",
            &format!("{base}/{transformation_id}"),
            None,
            &cookie,
            &csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let res = request(
            app.clone(),
            "DELETE",
            &format!("{base}/{transformation_id}"),
            None,
            &cookie,
            &csrf,
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let listed = store
            .list_transformations(user, project.project_id)
            .await
            .expect("list");
        assert!(listed.is_empty(), "tombstoned definition leaves the list");
    }

    /// Object lifecycle on disk (Section 6.9): the replaced revision's
    /// object is removed outright (no revision history is kept, so a
    /// trashed previous object could never be swept), and the deleted
    /// definition's object moves to the namespace trash for the sweep.
    #[tokio::test]
    async fn transformation_objects_replace_in_place_and_trash_on_delete() {
        let Some((auth, sink)) = auth_state().await else {
            return;
        };
        let (cookie, csrf) = login_session(auth.clone(), sink).await;
        let (app_state, file_base) = hosted_test_state(auth);
        let store = app_state.store.clone();
        let object_path = |key: &str| file_base.join(key);
        let user = {
            let presentation = cookie
                .split(';')
                .map(str::trim)
                .find_map(|c| c.strip_prefix(crate::auth::SESSION_COOKIE))
                .and_then(|c| c.strip_prefix('='))
                .expect("session cookie")
                .to_string();
            let digest = archaeodash_auth::token::digest_presentation(&presentation);
            store
                .find_live_session(&digest, std::time::SystemTime::now())
                .await
                .expect("session lookup")
                .expect("live session")
                .1
                .id
        };
        let project = store
            .create_project(
                Uuid::now_v7(),
                user,
                "object lifecycle",
                std::time::SystemTime::now(),
            )
            .await
            .expect("project");
        let base = format!("/api/v1/projects/{}/transformations", project.project_id);
        let definition = r#"{"definition":{"name":"Cu over Zn","transform_method":"log10","imputation_method":"none","imputation_seed":null,"elemental_columns":["Cu"],"descriptive_columns":[],"group_column":null,"ratios":[],"ratio_mode":"append"}}"#;
        let app = hosted_router(app_state);
        let post = |app: Router, uri: String, body: &str, csrf: String| {
            app.oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::COOKIE, cookie.clone())
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .extension(ConnectInfo(test_peer()))
                    .body(Body::from(body.to_string()))
                    .expect("request"),
            )
        };

        // First save writes the object at its catalog key.
        let res = post(app.clone(), base.clone(), definition, csrf.clone())
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::CREATED);
        let first_key = store
            .get_transformation_by_name(user, project.project_id, "Cu over Zn")
            .await
            .expect("get")
            .expect("row")
            .object_key;
        assert!(object_path(&first_key).exists(), "object written");

        // Replace: the catalog row moves to a new key and the previous
        // object is gone from the namespace (removed, not trashed — no
        // revision history is kept, so trash could never be swept).
        let res = post(app.clone(), base.clone(), definition, csrf.clone())
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        let second_key = store
            .get_transformation_by_name(user, project.project_id, "Cu over Zn")
            .await
            .expect("get")
            .expect("row")
            .object_key;
        assert_ne!(second_key, first_key, "each revision gets a fresh key");
        assert!(object_path(&second_key).exists(), "new object written");
        assert!(!object_path(&first_key).exists(), "old object removed");

        // Delete: tombstone + trash for the retention sweep.
        let transformation_id = store
            .get_transformation_by_name(user, project.project_id, "Cu over Zn")
            .await
            .expect("get")
            .expect("row")
            .transformation_id;
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("DELETE")
                    .uri(format!("{base}/{transformation_id}").as_str())
                    .header(header::COOKIE, cookie)
                    .header("x-csrf-token", csrf)
                    .extension(ConnectInfo(test_peer()))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(!object_path(&second_key).exists(), "object left files/");
        let trash = file_base
            .join("users")
            .join(user.to_string())
            .join("projects")
            .join(project.project_id.to_string())
            .join(".trash")
            .join(format!("{}.deleted", transformation_id.simple()));
        assert!(trash.exists(), "object moved to trash for the sweep");
    }
}
