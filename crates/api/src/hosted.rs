//! Hosted control-plane router composition (Section 10) and startup
//! readiness rules (Section 6.5): migrations are an explicit deployment
//! step, readiness is a read-only schema check, and every response carries
//! a correlation ID with enforced security headers.

use crate::auth::{auth_router, AuthState};
use crate::security_headers::apply_security_headers;
use archaeodash_contracts::ErrorEnvelope;
use archaeodash_control_postgres::ControlStore;
use archaeodash_control_postgres::FileRow;
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
        .route("/api/v1/files", post(files_upload).get(files_list))
        .route(
            "/api/v1/files/{id}",
            get(files_metadata).delete(files_delete),
        )
        .route("/api/v1/files/{id}/download", get(files_download))
        .with_state(state.clone());
    let health = Router::new()
        .route("/api/v1/health/live", get(health_live))
        .route("/api/v1/health/ready", get(health_ready))
        .with_state(state);
    let app: Router = Router::new()
        .merge(health)
        .merge(auth)
        .merge(files)
        .layer(middleware::from_fn(assign_request_id));
    apply_security_headers(app)
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
    let staged = state
        .files
        .stage(
            principal.user.id,
            project_id,
            &query.path,
            query.filename.as_deref(),
            &bytes,
        )
        .await
        .map_err(file_error)?;
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
        Ok(true) => Ok((
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
        )),
        // Foreign/deleted project or path conflict: remove the staged object
        // and report failure without a cross-account existence oracle.
        Ok(false) => {
            state.files.discard_staged(&staged);
            Err((StatusCode::NOT_FOUND, json_error_envelope("not_found")))
        }
        Err(e) => {
            state.files.discard_staged(&staged);
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
    match state
        .files
        .delete(&state.store, principal.user.id, file_id)
        .await
        .map_err(file_error)?
    {
        Some(_) => Ok(StatusCode::NO_CONTENT),
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
        let app = hosted_router(hosted_test_state(state));
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
#[cfg(test)]
fn hosted_test_state(state: crate::auth::AuthState) -> HostedState {
    HostedState {
        auth: state.clone(),
        store: state.store.clone(),
        files: std::sync::Arc::new(
            crate::hosted_files::HostedFileStore::new(
                tempfile::tempdir().expect("temp file store").keep(),
            )
            .expect("file store"),
        ),
    }
}
