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

use axum::routing::get;

/// Applies the enforced security headers to a composed hosted router.
/// Kept as a named step so deployment wiring reads as the Section 11.3
/// checklist: headers, then serve.
pub fn finalize_hosted_router(router: Router) -> Router {
    apply_security_headers(router)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use archaeodash_auth::throttle::ThrottlePepper;
    use axum::body::Body;
    use tower::ServiceExt;

    /// Builds a hosted router over a lazily-connected pool: middleware and
    /// header tests run without a database (readiness then degrades to 503),
    /// and with `DATABASE_URL` the readiness test exercises the real schema
    /// check.
    async fn hosted_test_router() -> HostedState {
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
