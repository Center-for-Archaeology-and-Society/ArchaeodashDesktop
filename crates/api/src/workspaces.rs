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
//!
//! Multi-user limits ([`WorkspaceLimits`]): a request body cap, a shared
//! (and per-user) cap on concurrent analysis requests, a small background
//! job pool per project, and a per-user storage quota checked before
//! requests that add data.

use crate::auth::{db_error, require_session};
use crate::hosted::{
    internal_error, json_error_envelope, quota_exceeded, unauthorized, HostedState,
};
use crate::{root_router, AppState};
use archaeodash_application::AnalysisJobs;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{Method, Request, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tower::ServiceExt;
use uuid::Uuid;

/// Route pattern of the project-scoped analysis API.
pub const WORKSPACE_ROUTE: &str = "/w/{project_id}/{*rest}";

/// Default number of project routers kept open at once.
pub const DEFAULT_OPEN_WORKSPACES: usize = 64;

/// Inner routes that run an analysis in the request (CPU-heavy). They share
/// the analysis slots; cheap reads (group scan, rows, validate) do not.
const ANALYSIS_ROUTES: &[&str] = &[
    "/api/v1/ordination/",
    "/api/v1/cluster/",
    "/api/v1/euclidean/",
    "/api/v1/explore/",
    "/api/v1/exports/",
    "/api/v1/imports/",
    "/api/v1/transformations/apply",
    "/api/v1/transformations/ratios/",
];

/// Inner `POST` routes that add data to a workspace. They are refused once
/// the user's storage reaches the quota; everything else (reads, analyses,
/// moves, deletes) keeps working so a user can always clean up.
const GROWING_ROUTES: &[&str] = &[
    "/api/v1/files",
    "/api/v1/imports/commit",
    "/api/v1/groups/duplicate",
    "/api/v1/groups/copy",
    "/api/v1/groups/apply-clusters",
    "/api/v1/groups/columns",
    "/api/v1/transformations",
];

pub(crate) fn is_analysis(path: &str) -> bool {
    ANALYSIS_ROUTES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

pub(crate) fn grows(method: &Method, path: &str) -> bool {
    method == Method::POST && GROWING_ROUTES.contains(&path)
}

/// Multi-user limits for hosted workspaces (set from env in the binary).
#[derive(Clone, Debug)]
pub struct WorkspaceLimits {
    /// Largest request body a workspace route accepts, uploads included.
    pub max_request_bytes: usize,
    /// Analysis requests running at once across all users.
    pub analysis_slots: usize,
    /// Analysis requests running at once for one user.
    pub analysis_slots_per_user: usize,
    /// How long an analysis request waits for a slot before a 503.
    pub analysis_wait: Duration,
    /// Background job threads per open project.
    pub job_workers: usize,
    /// Queued background jobs per open project.
    pub job_queue: usize,
}

impl Default for WorkspaceLimits {
    fn default() -> Self {
        let cores = std::thread::available_parallelism().map_or(2, usize::from);
        Self {
            max_request_bytes: 64 * 1024 * 1024,
            // Analyses run on the async runtime's threads; leave half free
            // for everything else.
            analysis_slots: (cores / 2).max(1),
            analysis_slots_per_user: 2,
            analysis_wait: Duration::from_secs(60),
            job_workers: 1,
            job_queue: 4,
        }
    }
}

/// Opens and caches one analysis router per hosted project, and holds the
/// shared limits.
///
/// Routers are closed least-recently-used first once more than `capacity`
/// are open. Closing drops that project's in-memory job list, so the
/// capacity should comfortably exceed the number of concurrently active
/// projects.
pub struct WorkspaceRegistry {
    base: PathBuf,
    capacity: usize,
    limits: WorkspaceLimits,
    open: Mutex<OpenWorkspaces>,
    analysis: Arc<Semaphore>,
    analysis_per_user: Mutex<HashMap<Uuid, Arc<Semaphore>>>,
    /// Anonymous guest sessions (`/g/{token}`), when enabled.
    guests: Option<Arc<crate::guests::GuestSessions>>,
}

#[derive(Default)]
struct OpenWorkspaces {
    routers: HashMap<(Uuid, Uuid), (Router, u64)>,
    clock: u64,
}

/// Both slots one analysis request holds while it runs.
pub struct AnalysisPermit {
    _user: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

impl WorkspaceRegistry {
    /// `base` is the user file store root (`AUTH_FILE_STORE_DIR`).
    pub fn new(base: impl Into<PathBuf>, capacity: usize) -> Self {
        Self::with_limits(base, capacity, WorkspaceLimits::default())
    }

    pub fn with_limits(base: impl Into<PathBuf>, capacity: usize, limits: WorkspaceLimits) -> Self {
        Self {
            base: base.into(),
            capacity: capacity.max(1),
            analysis: Arc::new(Semaphore::new(limits.analysis_slots.max(1))),
            limits,
            open: Mutex::new(OpenWorkspaces::default()),
            analysis_per_user: Mutex::new(HashMap::new()),
            guests: None,
        }
    }

    /// Enables anonymous guest sessions (see [`crate::guests`]).
    #[must_use]
    pub fn with_guests(mut self, guests: crate::guests::GuestSessions) -> Self {
        self.guests = Some(Arc::new(guests));
        self
    }

    /// The guest sessions, when enabled.
    pub fn guests(&self) -> Option<&Arc<crate::guests::GuestSessions>> {
        self.guests.as_ref()
    }

    /// Opens an analysis router on `dir` (created if missing) with this
    /// registry's job and body limits.
    pub(crate) fn open_router(&self, dir: &Path) -> Result<Router, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let dir = std::fs::canonicalize(dir).map_err(|e| e.to_string())?;
        let jobs = AnalysisJobs::with_limits(&dir, self.limits.job_workers, self.limits.job_queue);
        Ok(root_router(AppState::open_with_jobs(&dir, jobs)?)
            .layer(DefaultBodyLimit::max(self.limits.max_request_bytes)))
    }

    /// Drops the analysis slots kept for a user or guest that is gone.
    pub fn forget_slots(&self, id: Uuid) {
        if let Ok(mut per_user) = self.analysis_per_user.lock() {
            per_user.remove(&id);
        }
    }

    pub fn limits(&self) -> &WorkspaceLimits {
        &self.limits
    }

    fn user_dir(&self, user_id: Uuid) -> PathBuf {
        self.base.join("users").join(user_id.to_string())
    }

    /// The project directory of one hosted project.
    pub fn workspace_dir(&self, user_id: Uuid, project_id: Uuid) -> PathBuf {
        self.user_dir(user_id)
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
        let router = self.open_router(&self.workspace_dir(user_id, project_id))?;
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

    /// Waits (up to `analysis_wait`) for one of the user's analysis slots
    /// and one shared slot; `None` when the server stays busy.
    pub async fn analysis_permit(&self, user_id: Uuid) -> Option<AnalysisPermit> {
        let user_slots = {
            let mut per_user = self.analysis_per_user.lock().ok()?;
            per_user
                .entry(user_id)
                .or_insert_with(|| {
                    Arc::new(Semaphore::new(self.limits.analysis_slots_per_user.max(1)))
                })
                .clone()
        };
        let wait = self.limits.analysis_wait;
        let user = tokio::time::timeout(wait, user_slots.acquire_owned())
            .await
            .ok()?
            .ok()?;
        let global = tokio::time::timeout(wait, self.analysis.clone().acquire_owned())
            .await
            .ok()?
            .ok()?;
        Some(AnalysisPermit {
            _user: user,
            _global: global,
        })
    }

    /// Bytes stored in all of a user's project workspaces.
    pub async fn usage_bytes(&self, user_id: Uuid) -> u64 {
        let projects = self.user_dir(user_id).join("projects");
        tokio::task::spawn_blocking(move || {
            let Ok(entries) = std::fs::read_dir(&projects) else {
                return 0;
            };
            entries
                .flatten()
                .map(|project| tree_bytes(&project.path().join("workspace")))
                .sum()
        })
        .await
        .unwrap_or(0)
    }

    /// Closes a deleted project's router and removes its workspace folder.
    pub async fn remove(&self, user_id: Uuid, project_id: Uuid) {
        self.close(user_id, project_id);
        let dir = self.workspace_dir(user_id, project_id);
        let removed = tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        })
        .await;
        match removed {
            Ok(Ok(())) => {
                tracing::info!(project_id = %project_id, "removed deleted project's workspace")
            }
            Ok(Err(e)) => {
                tracing::error!(project_id = %project_id, error = %e, "workspace removal failed")
            }
            Err(e) => {
                tracing::error!(project_id = %project_id, error = %e, "workspace removal task failed")
            }
        }
    }
}

/// Total size of the regular files under `dir` (symlinks are not followed).
pub(crate) fn tree_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => tree_bytes(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
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

pub(crate) fn server_busy() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(archaeodash_contracts::ErrorEnvelope {
            code: "server_busy".into(),
            message: "The server is busy with other analyses. Try again in a moment.".into(),
        }),
    )
        .into_response()
}

/// Whether the user's stored bytes (workspaces plus catalog files) have
/// reached the quota.
async fn over_quota(
    state: &HostedState,
    user_id: Uuid,
) -> Result<bool, (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    let catalog = state
        .store
        .get_quota(user_id)
        .await
        .map_err(db_error)?
        .map_or(0, |(logical, reserved, _)| logical.saturating_add(reserved));
    let workspaces = i64::try_from(state.workspaces.usage_bytes(user_id).await).unwrap_or(i64::MAX);
    Ok(catalog.saturating_add(workspaces) >= state.quota_bytes)
}

/// Handler for [`WORKSPACE_ROUTE`]: session, CSRF and ownership checks, the
/// quota and analysis limits, then the request goes to the project's
/// analysis router unchanged except for its path.
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
    if grows(request.method(), inner_uri.path()) {
        match over_quota(&state, user_id).await {
            Ok(false) => {}
            Ok(true) => return quota_exceeded().into_response(),
            Err(rejection) => return rejection.into_response(),
        }
    }
    let router = match state.workspaces.router(user_id, project_id) {
        Ok(router) => router,
        Err(e) => {
            tracing::error!(error = %e, "workspace open failed");
            return internal_error("The project could not be opened.").into_response();
        }
    };
    // Held until the analysis response is ready.
    let _permit = if is_analysis(inner_uri.path()) {
        match state.workspaces.analysis_permit(user_id).await {
            Some(permit) => Some(permit),
            None => return server_busy(),
        }
    } else {
        None
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

/// The project ID of `DELETE /api/v1/projects/{id}`, if that is the request.
fn deleted_project(method: &Method, path: &str) -> Option<Uuid> {
    if method != Method::DELETE {
        return None;
    }
    let id = path.strip_prefix("/api/v1/projects/")?;
    Uuid::try_parse(id).ok()
}

/// Middleware: after a successful project delete (204), close the project's
/// router and remove its workspace folder. The delete itself (ownership,
/// CSRF, tombstone) stays with the catalog route.
pub(crate) async fn remove_deleted_workspace(
    State(state): State<HostedState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let Some(project_id) = deleted_project(request.method(), request.uri().path()) else {
        return next.run(request).await;
    };
    let user = match require_session(&state.auth, request.headers(), true).await {
        Ok(Some(principal)) => Some(principal.user.id),
        _ => None,
    };
    let response = next.run(request).await;
    if response.status() == StatusCode::NO_CONTENT {
        if let Some(user_id) = user {
            state.workspaces.remove(user_id, project_id).await;
        }
    }
    response
}

#[cfg(test)]
pub(crate) mod tests {
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
    pub(crate) fn offline_auth_state() -> AuthState {
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

    #[test]
    fn analysis_and_growing_routes_are_classified() {
        for path in [
            "/api/v1/ordination/pca",
            "/api/v1/cluster/fit",
            "/api/v1/euclidean/matches",
            "/api/v1/explore/histogram",
            "/api/v1/exports/table",
            "/api/v1/imports/commit",
            "/api/v1/transformations/apply",
        ] {
            assert!(is_analysis(path), "{path}");
        }
        for path in [
            "/api/v1/groups",
            "/api/v1/groups/rows",
            "/api/v1/groups/validate",
            "/api/v1/jobs",
        ] {
            assert!(!is_analysis(path), "{path}");
        }
        assert!(grows(&Method::POST, "/api/v1/files"));
        assert!(grows(&Method::POST, "/api/v1/imports/commit"));
        assert!(grows(&Method::POST, "/api/v1/groups/duplicate"));
        assert!(!grows(&Method::DELETE, "/api/v1/files/x"));
        assert!(!grows(&Method::POST, "/api/v1/groups/merge"));
        assert!(!grows(&Method::POST, "/api/v1/ordination/pca"));
    }

    #[tokio::test]
    async fn analysis_slots_cap_each_user_and_the_server() {
        let base = tempfile::tempdir().expect("base");
        let limits = WorkspaceLimits {
            analysis_slots: 2,
            analysis_slots_per_user: 1,
            analysis_wait: Duration::from_millis(50),
            ..WorkspaceLimits::default()
        };
        let registry = WorkspaceRegistry::with_limits(base.path(), 4, limits);
        let (a, b, c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let a1 = registry.analysis_permit(a).await.expect("a first");
        assert!(registry.analysis_permit(a).await.is_none(), "per-user cap");
        let b1 = registry.analysis_permit(b).await.expect("b first");
        assert!(registry.analysis_permit(c).await.is_none(), "server cap");
        drop(a1);
        let c1 = registry.analysis_permit(c).await.expect("slot freed");
        drop((b1, c1));
    }

    #[tokio::test]
    async fn usage_counts_only_that_users_workspaces() {
        let base = tempfile::tempdir().expect("base");
        let registry = WorkspaceRegistry::new(base.path(), 4);
        let (user, other) = (Uuid::now_v7(), Uuid::now_v7());
        let (p1, p2, p3) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let write = |owner: Uuid, project: Uuid, rel: &str, bytes: usize| {
            let path = registry.workspace_dir(owner, project).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![b'x'; bytes]).unwrap();
        };
        write(user, p1, "groups/a.parquet", 100);
        write(user, p1, ".archaeodash/cache/c.bin", 20);
        write(user, p2, "sources/s.csv", 5);
        write(other, p3, "groups/b.parquet", 1000);
        assert_eq!(registry.usage_bytes(user).await, 125);
        assert_eq!(registry.usage_bytes(other).await, 1000);
        assert_eq!(registry.usage_bytes(Uuid::now_v7()).await, 0);
        registry.remove(user, p1).await;
        assert!(!registry.workspace_dir(user, p1).exists());
        assert_eq!(registry.usage_bytes(user).await, 5);
    }

    #[tokio::test]
    async fn workspace_requests_are_held_to_the_body_limit() {
        let base = tempfile::tempdir().expect("base");
        let limits = WorkspaceLimits {
            max_request_bytes: 16,
            ..WorkspaceLimits::default()
        };
        let registry = WorkspaceRegistry::with_limits(base.path(), 4, limits);
        let router = registry
            .router(Uuid::now_v7(), Uuid::now_v7())
            .expect("router");
        let send = |body: &'static str| {
            router.clone().oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/files?path=sources/a.csv")
                    .body(Body::from(body))
                    .expect("request"),
            )
        };
        let response = send("a,b\n1,2\n3,4\n5,6\n7,8\n9,0\n")
            .await
            .expect("infallible");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let response = send("a,b\n1,2\n").await.expect("infallible");
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// DB-backed (skips without `DATABASE_URL`): once a user's storage
    /// reaches the quota, uploads are refused while reads keep working; the
    /// quota route reports workspace bytes; deleting the project removes its
    /// workspace.
    #[tokio::test]
    async fn quota_refuses_new_data_and_project_delete_removes_the_workspace() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let (cookie, csrf) = login_session(state.clone(), sink).await;
        let (mut hosted, base) = hosted_test_state(state);
        hosted.quota_bytes = 1;
        let app = hosted_router(hosted);
        let response = call(
            &app,
            "POST",
            "/api/v1/projects",
            Some(&cookie),
            Some(&csrf),
            serde_json::json!({ "name": "quota" }).to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let project = json(response).await["project_id"]
            .as_str()
            .expect("id")
            .to_string();
        let upload = |path: &str| format!("/w/{project}/api/v1/files?path={path}");
        // Empty workspace: under quota, so the first upload goes through.
        let response = call(
            &app,
            "POST",
            &upload("sources/a.csv"),
            Some(&cookie),
            Some(&csrf),
            "a,b\n1,2\n",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let response = call(
            &app,
            "POST",
            &upload("sources/b.csv"),
            Some(&cookie),
            Some(&csrf),
            "a,b\n1,2\n",
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(json(response).await["code"], "quota_exceeded");
        let groups = format!("/w/{project}/api/v1/groups");
        let response = call(&app, "GET", &groups, Some(&cookie), None, Body::empty()).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "reads keep working over quota"
        );
        let response = call(
            &app,
            "GET",
            "/api/v1/quota",
            Some(&cookie),
            None,
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            json(response).await["workspace_bytes"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );

        let workspace: Vec<PathBuf> = std::fs::read_dir(base.join("users"))
            .expect("users")
            .flatten()
            .map(|user| {
                user.path()
                    .join("projects")
                    .join(&project)
                    .join("workspace")
            })
            .filter(|dir| dir.is_dir())
            .collect();
        assert_eq!(workspace.len(), 1);
        let response = call(
            &app,
            "DELETE",
            &format!("/api/v1/projects/{project}"),
            Some(&cookie),
            Some(&csrf),
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(!workspace[0].exists(), "workspace removed with the project");
        let response = call(&app, "GET", &groups, Some(&cookie), None, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn analysis_requests_cannot_reach_another_workspace() {
        let base = tempfile::tempdir().expect("base");
        let registry = WorkspaceRegistry::new(base.path(), 4);
        let (user, other) = (Uuid::now_v7(), Uuid::now_v7());
        let (mine, theirs) = (Uuid::now_v7(), Uuid::now_v7());
        // The other user's group file exists, so only the path check stops it.
        let target = registry
            .workspace_dir(other, theirs)
            .join("groups/secret.parquet");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"PAR1").unwrap();
        let router = registry.router(user, mine).expect("router");
        let traversal =
            format!("../../../../{other}/projects/{theirs}/workspace/groups/secret.parquet");
        let absolute = target.to_str().expect("utf8").to_string();
        for (route, path) in [
            ("/api/v1/ordination/pca", &traversal),
            ("/api/v1/ordination/pca", &absolute),
            ("/api/v1/exports/measured-data", &traversal),
        ] {
            let body = if route.starts_with("/api/v1/exports/") {
                serde_json::json!({ "path": path, "extra_paths": [], "raw_text": false })
            } else {
                serde_json::json!({
                    "path": path,
                    "extra_paths": [],
                    "columns": ["Ti", "Sr"],
                    "transformation": null,
                })
            };
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(route)
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .expect("request"),
                )
                .await
                .expect("infallible");
            let status = response.status();
            let text = String::from_utf8_lossy(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body"),
            )
            .to_string();
            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{route} {path}: {text}"
            );
            assert!(
                text.contains("path_outside_project"),
                "{route} {path}: {text}"
            );
            // The refusal may repeat what the client sent, but never adds a
            // server path the client did not already supply.
            if *path != absolute {
                assert!(
                    !text.contains(base.path().to_str().unwrap()),
                    "no server paths: {text}"
                );
            }
        }
    }
}
