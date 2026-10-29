//! Hosted per-project workspaces: the analysis API behind hosted sign-in.
//!
//! Each hosted project owns a real project directory inside the user file
//! store, `users/<user-id>/projects/<project-id>/workspace/`, so every
//! analysis use case runs unchanged against it (Parquet groups, folders,
//! the project lock, the `.archaeodash/` cache). Only opaque UUIDs reach
//! the filesystem.
//!
//! The analysis API is served at `/w/{project_id}/api/v1/...`. A request
//! is forwarded to that project's router only with a live session, the
//! CSRF token on state-changing methods, and ownership of the project.
//! Other users' projects, deleted projects and unknown IDs are all 404, so
//! the route is no existence oracle across accounts. The project ID is a
//! path prefix rather than a header because `EventSource` (job progress)
//! cannot send headers.

use crate::auth::{db_error, require_session};
use crate::hosted::{internal_error, json_error_envelope, unauthorized, HostedState};
use crate::{root_router, AppState};
use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

/// Route pattern of the project-scoped analysis API.
pub const WORKSPACE_ROUTE: &str = "/w/{project_id}/{*rest}";

/// Default number of project routers kept open at once.
pub const DEFAULT_OPEN_WORKSPACES: usize = 64;

/// Opens and caches one analysis router per hosted project.
///
/// Routers are closed least-recently-used first once more than `capacity`
/// are open. Closing drops that project's in-memory job list, so the
/// capacity should comfortably exceed the number of concurrently active
/// projects.
pub struct WorkspaceRegistry {
    base: PathBuf,
    capacity: usize,
    open: Mutex<OpenWorkspaces>,
}

#[derive(Default)]
struct OpenWorkspaces {
    routers: HashMap<(Uuid, Uuid), (Router, u64)>,
    clock: u64,
}

impl WorkspaceRegistry {
    /// `base` is the user file store root (`AUTH_FILE_STORE_DIR`).
    pub fn new(base: impl Into<PathBuf>, capacity: usize) -> Self {
        Self {
            base: base.into(),
            capacity: capacity.max(1),
            open: Mutex::new(OpenWorkspaces::default()),
        }
    }

    /// The project directory of one hosted project.
    pub fn workspace_dir(&self, user_id: Uuid, project_id: Uuid) -> PathBuf {
        self.base
            .join("users")
            .join(user_id.to_string())
            .join("projects")
            .join(project_id.to_string())
            .join("workspace")
    }

    /// The project's analysis router, creating and opening the workspace on
    /// first use. Callers must have checked ownership already.
    pub fn router(&self, user_id: Uuid, project_id: Uuid) -> Result<Router, String> {
        let mut open = self
            .open
            .lock()
            .map_err(|_| "workspace registry lock poisoned".to_string())?;
        open.clock += 1;
        let now = open.clock;
        let key = (user_id, project_id);
        if let Some((router, used)) = open.routers.get_mut(&key) {
            *used = now;
            return Ok(router.clone());
        }
        // Opening happens under the lock so one project never gets two
        // service sets (two job lists) from concurrent first requests.
        let dir = self.workspace_dir(user_id, project_id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let router = root_router(AppState::open(&dir)?);
        if open.routers.len() >= self.capacity {
            let oldest = open
                .routers
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                open.routers.remove(&oldest);
            }
        }
        open.routers.insert(key, (router.clone(), now));
        Ok(router)
    }

    /// Closes a project's router, e.g. after the project is deleted.
    pub fn close(&self, user_id: Uuid, project_id: Uuid) {
        if let Ok(mut open) = self.open.lock() {
            open.routers.remove(&(user_id, project_id));
        }
    }

    /// Number of project routers currently open.
    pub fn open_count(&self) -> usize {
        self.open.lock().map(|open| open.routers.len()).unwrap_or(0)
    }
}

/// Splits `/w/{project_id}/api/v1/...?query` into the project ID and the
/// inner `/api/v1/...?query` URI. The raw (still percent-encoded) path is
/// kept so wildcard routes such as group delete see exactly what the
/// client sent. Anything outside `/api/v1/` is refused.
fn split_workspace_uri(uri: &Uri) -> Option<(Uuid, Uri)> {
    let rest = uri.path().strip_prefix("/w/")?;
    let (id, inner) = rest.split_once('/')?;
    let project_id = Uuid::try_parse(id).ok()?;
    if !inner.starts_with("api/v1/") {
        return None;
    }
    let mut target = format!("/{inner}");
    if let Some(query) = uri.query() {
        target.push('?');
        target.push_str(query);
    }
    target.parse().ok().map(|inner| (project_id, inner))
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, json_error_envelope("not_found")).into_response()
}

/// Handler for [`WORKSPACE_ROUTE`]: session, CSRF and ownership checks,
/// then the request goes to the project's analysis router unchanged except
/// for its path.
pub(crate) async fn workspace_dispatch(
    State(state): State<HostedState>,
    request: Request<Body>,
) -> Response {
    let writes = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let principal = match require_session(&state.auth, request.headers(), writes).await {
        Ok(Some(principal)) => principal,
        Ok(None) => return unauthorized().into_response(),
        Err(rejection) => return rejection.into_response(),
    };
    let Some((project_id, inner_uri)) = split_workspace_uri(request.uri()) else {
        return not_found();
    };
    let user_id = principal.user.id;
    match state.store.get_project(user_id, project_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(),
        Err(e) => return db_error(e).into_response(),
    }
    let router = match state.workspaces.router(user_id, project_id) {
        Ok(router) => router,
        Err(e) => {
            tracing::error!(error = %e, "workspace open failed");
            return internal_error("The project could not be opened.").into_response();
        }
    };
    let (mut parts, body) = request.into_parts();
    parts.uri = inner_uri;
    // Drop the outer route's matched path parameters (`project_id`, `rest`):
    // axum would otherwise hand them to inner `Path` extractors too.
    parts.extensions = axum::http::Extensions::new();
    match router.oneshot(Request::from_parts(parts, body)).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)] // test code; panics are the failure mode

    use super::*;
    use crate::auth::preference_tests::login_session;
    use crate::auth::tests::{auth_state, test_peer};
    use crate::auth::{AuthState, CSRF_COOKIE, SESSION_COOKIE};
    use crate::hosted::hosted_test_state;
    use crate::hosted_router;
    use archaeodash_auth::throttle::ThrottlePepper;
    use archaeodash_control_postgres::ControlStore;
    use axum::extract::ConnectInfo;
    use axum::http::header;
    use std::sync::Arc;

    #[test]
    fn workspace_uri_split_keeps_raw_path_and_query() {
        let id = Uuid::now_v7();
        let uri: Uri = format!("/w/{id}/api/v1/groups/a%2Fb.parquet?x=1")
            .parse()
            .unwrap();
        let (project, inner) = split_workspace_uri(&uri).expect("split");
        assert_eq!(project, id);
        assert_eq!(inner.path(), "/api/v1/groups/a%2Fb.parquet");
        assert_eq!(inner.query(), Some("x=1"));
    }

    #[test]
    fn workspace_uri_split_refuses_non_api_paths_and_bad_ids() {
        let id = Uuid::now_v7();
        for bad in [
            format!("/w/{id}/healthz"),
            format!("/w/{id}/"),
            format!("/w/{id}"),
            "/w/not-a-uuid/api/v1/groups".to_string(),
            "/api/v1/groups".to_string(),
        ] {
            let uri: Uri = bad.parse().unwrap();
            assert!(split_workspace_uri(&uri).is_none(), "{bad} must not split");
        }
    }

    #[test]
    fn registry_creates_workspaces_and_closes_least_recently_used() {
        let base = tempfile::tempdir().expect("temp base");
        let registry = WorkspaceRegistry::new(base.path(), 2);
        let user = Uuid::now_v7();
        let (p1, p2, p3) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        registry.router(user, p1).expect("p1");
        registry.router(user, p2).expect("p2");
        registry.router(user, p1).expect("p1 again"); // p2 is now the oldest
        registry.router(user, p3).expect("p3");
        assert_eq!(registry.open_count(), 2);
        for project in [p1, p2, p3] {
            assert!(registry.workspace_dir(user, project).is_dir());
        }
        let expected = base
            .path()
            .join("users")
            .join(user.to_string())
            .join("projects")
            .join(p1.to_string())
            .join("workspace");
        assert_eq!(registry.workspace_dir(user, p1), expected);
        // p2 was closed; reopening works and evicts the next oldest.
        registry.router(user, p2).expect("p2 reopened");
        assert_eq!(registry.open_count(), 2);
        registry.close(user, p2);
        assert_eq!(registry.open_count(), 1);
    }

    /// Auth state over a lazily connected pool: requests rejected before
    /// any database query (no session, missing CSRF) need no database.
    fn offline_auth_state() -> AuthState {
        let pool = sqlx::PgPool::connect_lazy("postgres://nobody:nopass@127.0.0.1:9/nodb")
            .expect("lazy pool builds");
        AuthState {
            store: Arc::new(ControlStore::new(pool)),
            email: Arc::new(archaeodash_auth::email::DevSinkEmailSender::new()),
            pepper: ThrottlePepper::from_hex(&"ab".repeat(32)).expect("pepper"),
            base_url: "https://archaeodash.example".to_string(),
        }
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        csrf: Option<&str>,
        body: impl Into<Body>,
    ) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            // JSON routes require it; byte-upload routes ignore it.
            .header(header::CONTENT_TYPE, "application/json")
            .extension(ConnectInfo(test_peer()));
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = csrf {
            request = request.header("x-csrf-token", csrf);
        }
        app.clone()
            .oneshot(request.body(body.into()).expect("request"))
            .await
            .expect("infallible")
    }

    async fn json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn workspace_routes_require_a_session() {
        let (state, _base) = hosted_test_state(offline_auth_state());
        let app = hosted_router(state);
        let uri = format!("/w/{}/api/v1/groups", Uuid::now_v7());
        let response = call(&app, "GET", &uri, None, None, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = call(&app, "POST", &uri, None, None, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn workspace_writes_require_the_csrf_token() {
        let (state, _base) = hosted_test_state(offline_auth_state());
        let app = hosted_router(state);
        let cookie = format!("{SESSION_COOKIE}=s; {CSRF_COOKIE}=c");
        let uri = format!("/w/{}/api/v1/files?path=a.csv", Uuid::now_v7());
        let response = call(&app, "POST", &uri, Some(&cookie), None, "a,b\n1,2\n").await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = call(&app, "POST", &uri, Some(&cookie), Some("wrong"), "a,b\n").await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// DB-backed (skips without `DATABASE_URL`): an owner's workspace works
    /// and is separate per project; another account gets 404 for reads and
    /// writes, as does an unknown project.
    #[tokio::test]
    async fn workspaces_are_private_to_their_owner_and_project() {
        let Some((state_a, sink_a)) = auth_state().await else {
            return;
        };
        let Some((state_b, sink_b)) = auth_state().await else {
            return;
        };
        let (cookie_a, csrf_a) = login_session(state_a.clone(), sink_a).await;
        let (cookie_b, csrf_b) = login_session(state_b, sink_b).await;
        let (hosted, base) = hosted_test_state(state_a);
        let app = hosted_router(hosted);

        let mut projects = Vec::new();
        for name in ["first", "second"] {
            let response = call(
                &app,
                "POST",
                "/api/v1/projects",
                Some(&cookie_a),
                Some(&csrf_a),
                serde_json::json!({ "name": name }).to_string(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::CREATED);
            let id = json(response).await["project_id"]
                .as_str()
                .expect("project id")
                .to_string();
            projects.push(id);
        }
        let (p1, p2) = (&projects[0], &projects[1]);

        // The owner can scan and upload into the first project.
        let groups = format!("/w/{p1}/api/v1/groups");
        let response = call(&app, "GET", &groups, Some(&cookie_a), None, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let upload = format!("/w/{p1}/api/v1/files?path=sources/a.csv");
        let response = call(
            &app,
            "POST",
            &upload,
            Some(&cookie_a),
            Some(&csrf_a),
            "a,b\n1,2\n",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let file_id = json(response).await["file_id"]
            .as_str()
            .expect("file id")
            .to_string();
        let in_p1 = format!("/w/{p1}/api/v1/files/{file_id}");
        let response = call(&app, "GET", &in_p1, Some(&cookie_a), None, Body::empty()).await;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );

        // The second project is a separate workspace.
        let in_p2 = format!("/w/{p2}/api/v1/files/{file_id}");
        let response = call(&app, "GET", &in_p2, Some(&cookie_a), None, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Another account sees nothing, for reads and writes alike.
        for (method, uri) in [("GET", &groups), ("GET", &in_p1), ("DELETE", &in_p1)] {
            let response = call(
                &app,
                method,
                uri,
                Some(&cookie_b),
                Some(&csrf_b),
                Body::empty(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {uri}");
        }
        let response = call(
            &app,
            "POST",
            &upload,
            Some(&cookie_b),
            Some(&csrf_b),
            "a,b\n",
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // An unknown project is 404 for the owner too.
        let unknown = format!("/w/{}/api/v1/groups", Uuid::now_v7());
        let response = call(&app, "GET", &unknown, Some(&cookie_a), None, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Exactly the first project's workspace holds the upload, and the
        // rejected requests created no workspace for anyone else.
        let workspaces: Vec<PathBuf> = std::fs::read_dir(base.join("users"))
            .expect("users dir")
            .flat_map(|user| {
                std::fs::read_dir(user.expect("entry").path().join("projects"))
                    .into_iter()
                    .flatten()
                    .map(|project| project.expect("entry").path().join("workspace"))
            })
            .filter(|dir| dir.is_dir())
            .collect();
        assert_eq!(workspaces.len(), 2, "{workspaces:?}");
        assert!(workspaces
            .iter()
            .any(|dir| dir.ends_with(format!("{p1}/workspace"))));
        assert!(workspaces
            .iter()
            .any(|dir| dir.ends_with(format!("{p2}/workspace"))));
    }
}
