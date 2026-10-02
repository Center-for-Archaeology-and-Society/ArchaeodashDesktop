//! Hosted security headers and enforced CSP (Section 11.3, Section 13
//! `security/apache-hardening.conf` disposition: "enforce tested CSP rather
//! than report-only; keep HSTS/nosniff/referrer/permissions/frame protection
//! intent").
//!
//! The hosted web bundle ships only locally bundled JS/CSS/fonts, so the CSP
//! allows no remote origins and no `unsafe-eval`. Inline `style-src`
//! attributes are permitted because the UI framework applies layout styles
//! as element attributes at runtime; script execution stays `'self'`-only.

use axum::http::{header, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use axum::Router;

/// Enforced Content-Security-Policy for the hosted UI (Section 11.3: no
/// `unsafe-eval`, no remote script execution).
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; \
     base-uri 'self'; \
     object-src 'none'; \
     frame-ancestors 'self'; \
     form-action 'self'; \
     script-src 'self'; \
     style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; \
     font-src 'self'; \
     connect-src 'self'";

/// Header name/value pairs applied to every hosted response.
pub fn security_headers() -> Vec<(HeaderName, &'static str)> {
    vec![
        (header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY),
        (
            header::STRICT_TRANSPORT_SECURITY,
            "max-age=31536000; includeSubDomains",
        ),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "strict-origin-when-cross-origin"),
        (
            HeaderName::from_static("permissions-policy"),
            "geolocation=(), microphone=(), camera=()",
        ),
        (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
    ]
}

/// Tower middleware that stamps the security headers onto every response,
/// overriding any same-named header an inner handler may have set.
pub async fn stamp_security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (name, value) in security_headers() {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

/// Wraps a router with the security-headers middleware (Section 10 hosted
/// router composition point).
pub fn apply_security_headers(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(stamp_security_headers))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::routing::get;

    #[tokio::test]
    async fn every_response_carries_all_hardening_headers() {
        use tower::ServiceExt;
        let app = apply_security_headers(Router::new().route("/anything", get(|| async { "ok" })));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/anything")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        for (name, value) in security_headers() {
            let got = response
                .headers()
                .get(&name)
                .unwrap_or_else(|| panic!("missing header {name:?}"));
            assert_eq!(got, value, "header {name:?}");
        }
    }

    #[tokio::test]
    async fn inner_headers_are_overridden_not_duplicated() {
        use tower::ServiceExt;
        let app = apply_security_headers(Router::new().route(
            "/x",
            get(|| async {
                let mut response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .expect("build");
                response.headers_mut().insert(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("sniff"),
                );
                response
            }),
        ));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/x")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        let values: Vec<&str> = response
            .headers()
            .get_all(header::X_CONTENT_TYPE_OPTIONS)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        assert_eq!(values, vec!["nosniff"]);
    }

    #[test]
    fn csp_has_no_unsafe_eval_and_no_remote_scripts() {
        assert!(!CONTENT_SECURITY_POLICY.contains("unsafe-eval"));
        assert!(!CONTENT_SECURITY_POLICY.contains("http"));
        assert!(CONTENT_SECURITY_POLICY.contains("script-src 'self'"));
        assert!(CONTENT_SECURITY_POLICY.contains("frame-ancestors 'self'"));
    }
}
