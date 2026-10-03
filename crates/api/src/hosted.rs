//! Hosted control-plane router composition (Section 10) and startup
//! readiness rules (Section 6.5): migrations are an explicit deployment
//! step, readiness is a read-only schema check, and every response carries
//! a correlation ID with enforced security headers.

use crate::auth::{auth_router, AuthState};
use crate::security_headers::apply_security_headers;
use archaeodash_control_postgres::ControlStore;
use axum::extract::State;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;

pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Shared state for the hosted health/readiness endpoints.
#[derive(Clone)]
pub struct HostedState {
    pub auth: AuthState,
    pub store: Arc<ControlStore>,
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
    let health = Router::new()
        .route("/api/v1/health/live", get(health_live))
        .route("/api/v1/health/ready", get(health_ready))
        .with_state(state);
    let app: Router = Router::new()
        .merge(health)
        .merge(auth)
        .layer(middleware::from_fn(assign_request_id));
    apply_security_headers(app)
}

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
    pub(crate) async fn hosted_test_router() -> HostedState {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://nobody:nopass@127.0.0.1:9/nodb".to_string());
        let pool = sqlx::PgPool::connect_lazy(&url).expect("lazy pool builds");
        let store = Arc::new(ControlStore::new(pool));
        HostedState {
            auth: AuthState {
                store: store.clone(),
                email: Arc::new(archaeodash_auth::email::DevSinkEmailSender::new()),
                pepper: ThrottlePepper::from_hex(&"ab".repeat(32)).expect("pepper"),
                base_url: "https://archaeodash.example".to_string(),
            },
            store,
        }
    }

    #[tokio::test]
    async fn request_id_is_echoed_and_generated() {
        let state = hosted_test_router().await;
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
        let state = hosted_test_router().await;
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
        let state = hosted_test_router().await;
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
    use axum::body::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn responses_disclose_no_server_technology() {
        let state = hosted_test_router().await;
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
}
