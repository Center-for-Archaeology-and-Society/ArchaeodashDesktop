//! Axum HTTP API composition (Section 10).
//!
//! Phase 1 smoke surface: a single health endpoint proving the shared
//! application use case executes through the web transport adapter.

use archaeodash_application::app_info;
use archaeodash_contracts::AppInfo;
use axum::routing::get;
use axum::Router;

async fn healthz() -> axum::Json<AppInfo> {
    axum::Json(app_info("http", true))
}

/// Builds the root router. Route groups for import, groups, transformations,
/// analysis, jobs, and auth land in their owning phases (Sections 10.1+).
pub fn root_router() -> Router {
    Router::new().route("/healthz", get(healthz))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_reports_ready_over_http() {
        let response = root_router()
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
}
