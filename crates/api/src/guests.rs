//! Anonymous guest workspaces: the analysis API without an account.
//!
//! `POST /api/v1/guest` creates a temporary workspace in the guest directory
//! and returns a random token. The token is the only key to it: the web
//! client keeps it in page memory, so the data can be reached only while
//! that page stays open. Requests go to `/g/{token}/api/v1/...` (a path
//! prefix, like `/w/{project_id}`, because `EventSource` cannot send
//! headers). No cookie is involved, so there is no cross-site request
//! forgery exposure, and nothing is stored in the database.
//!
//! Cleanup ([`GuestLimits`]): the page ends its session when it closes
//! (`POST /api/v1/guest/end`) and pings while open (`/api/v1/guest/ping`).
//! A sweep removes sessions idle longer than `idle` or older than
//! `max_age`, then the least recently used ones while all guest data
//! exceeds `total_bytes`. It runs periodically, before each new session and
//! before each request that adds data. The guest directory is emptied at
//! startup: tokens live only in memory, so earlier sessions are unreachable.

use crate::hosted::{internal_error, quota_exceeded, HostedState};
use crate::workspaces::{grows, is_analysis, server_busy, tree_bytes, WorkspaceRegistry};
use archaeodash_contracts::ErrorEnvelope;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower::ServiceExt;
use uuid::Uuid;

/// Route pattern of the guest-scoped analysis API.
pub const GUEST_ROUTE: &str = "/g/{token}/{*rest}";

/// Marks a directory as the guest directory, so startup only ever empties a
/// directory this code created (never, say, a mistyped user-file root).
const MARKER: &str = ".archaeodash-guests";

/// Limits and cleanup policy for guest sessions (set from env in the binary).
#[derive(Clone, Debug)]
pub struct GuestLimits {
    /// Storage one guest session may use.
    pub quota_bytes: u64,
    /// A session without requests or pings for this long is removed.
    pub idle: Duration,
    /// A session is removed this long after it started, whatever its use.
    pub max_age: Duration,
    /// All guest data together; above it the least recently used sessions
    /// are removed.
    pub total_bytes: u64,
    /// Guest sessions at once; new ones are refused beyond it.
    pub max_sessions: usize,
    /// New sessions per client address per hour.
    pub starts_per_ip_per_hour: u32,
}

impl Default for GuestLimits {
    fn default() -> Self {
        Self {
            quota_bytes: 200 * 1024 * 1024,
            idle: Duration::from_secs(2 * 60 * 60),
            max_age: Duration::from_secs(24 * 60 * 60),
            total_bytes: 5 * 1024 * 1024 * 1024,
            max_sessions: 100,
            starts_per_ip_per_hour: 10,
        }
    }
}

struct GuestEntry {
    id: Uuid,
    dir: PathBuf,
    router: Router,
    created: Instant,
    last_seen: Instant,
}

#[derive(Default)]
struct GuestTable {
    /// Keyed by the SHA-256 of the token; the token itself is not kept.
    entries: HashMap<[u8; 32], GuestEntry>,
    /// Start of the current hour window and starts in it, per address.
    starts: HashMap<IpAddr, (Instant, u32)>,
}

/// Why a guest session could not start.
#[derive(Debug)]
pub enum GuestStartError {
    RateLimited,
    Full,
    Internal(String),
}

/// A session removed by `end` or a sweep: its folder still has to go.
pub struct RemovedGuest {
    id: Uuid,
    root: PathBuf,
}

/// The guest sessions of one server process.
pub struct GuestSessions {
    base: PathBuf,
    limits: GuestLimits,
    table: Mutex<GuestTable>,
}

fn token_key(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Tokens are URL-safe base64; anything else is refused before hashing.
fn plausible_token(token: &str) -> bool {
    (20..=128).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl GuestSessions {
    /// Uses `base` as the guest directory, emptying it first. Refuses a
    /// non-empty directory this code did not create.
    pub fn new(base: impl Into<PathBuf>, limits: GuestLimits) -> Result<Self, String> {
        let base = base.into();
        std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        let marker = base.join(MARKER);
        let mut entries = std::fs::read_dir(&base)
            .map_err(|e| e.to_string())?
            .flatten()
            .peekable();
        if entries.peek().is_some() && !marker.exists() {
            return Err(format!(
                "guest directory {} is not empty and was not created for guests",
                base.display()
            ));
        }
        for entry in entries {
            if entry.file_name() == MARKER {
                continue;
            }
            let path = entry.path();
            let removed = match entry.file_type() {
                Ok(kind) if kind.is_dir() => std::fs::remove_dir_all(&path),
                _ => std::fs::remove_file(&path),
            };
            removed.map_err(|e| format!("emptying {}: {e}", path.display()))?;
        }
        std::fs::write(&marker, b"").map_err(|e| e.to_string())?;
        Ok(Self {
            base,
            limits,
            table: Mutex::new(GuestTable::default()),
        })
    }

    pub fn limits(&self) -> &GuestLimits {
        &self.limits
    }

    /// Number of live guest sessions.
    pub fn count(&self) -> usize {
        self.table.lock().map(|t| t.entries.len()).unwrap_or(0)
    }

    /// Starts a session for a client at `ip` and returns its token.
    pub fn create(
        &self,
        registry: &WorkspaceRegistry,
        ip: IpAddr,
    ) -> Result<String, GuestStartError> {
        let now = Instant::now();
        let mut table = self
            .table
            .lock()
            .map_err(|_| GuestStartError::Internal("guest table lock poisoned".into()))?;
        let window = table.starts.entry(ip).or_insert((now, 0));
        if now.duration_since(window.0) >= Duration::from_secs(3600) {
            *window = (now, 0);
        }
        if window.1 >= self.limits.starts_per_ip_per_hour {
            return Err(GuestStartError::RateLimited);
        }
        if table.entries.len() >= self.limits.max_sessions {
            return Err(GuestStartError::Full);
        }
        let token = archaeodash_auth::token::OpaqueToken::generate()
            .map_err(|e| GuestStartError::Internal(e.to_string()))?
            .presentation()
            .to_string();
        let id = Uuid::now_v7();
        let root = self.base.join(id.to_string());
        let dir = root.join("workspace");
        let router = registry
            .open_router(&dir)
            .map_err(GuestStartError::Internal)?;
        if let Some(window) = table.starts.get_mut(&ip) {
            window.1 += 1;
        }
        table.entries.insert(
            token_key(&token),
            GuestEntry {
                id,
                dir,
                router,
                created: now,
                last_seen: now,
            },
        );
        Ok(token)
    }

    /// The session's ID, folder and router, marking it used.
    fn lookup(&self, token: &str) -> Option<(Uuid, PathBuf, Router)> {
        if !plausible_token(token) {
            return None;
        }
        let mut table = self.table.lock().ok()?;
        let entry = table.entries.get_mut(&token_key(token))?;
        entry.last_seen = Instant::now();
        Some((entry.id, entry.dir.clone(), entry.router.clone()))
    }

    /// Marks a session used; false when it no longer exists.
    pub fn touch(&self, token: &str) -> bool {
        self.lookup(token).is_some()
    }

    /// Ends a session; its folder is removed by [`Self::remove_folders`].
    pub fn end(&self, token: &str) -> Option<RemovedGuest> {
        if !plausible_token(token) {
            return None;
        }
        let mut table = self.table.lock().ok()?;
        let entry = table.entries.remove(&token_key(token))?;
        Some(RemovedGuest {
            id: entry.id,
            root: self.base.join(entry.id.to_string()),
        })
    }

    /// Removes expired sessions, then the least recently used ones while
    /// all guest data exceeds `total_bytes` (down to 80% of it). Blocking:
    /// it sizes folders on disk.
    pub fn sweep(&self) -> Vec<RemovedGuest> {
        let now = Instant::now();
        let mut removed = Vec::new();
        let snapshot: Vec<([u8; 32], PathBuf, Instant)> = {
            let Ok(mut table) = self.table.lock() else {
                return removed;
            };
            let limits = &self.limits;
            let expired: Vec<[u8; 32]> = table
                .entries
                .iter()
                .filter(|(_, e)| {
                    now.duration_since(e.last_seen) > limits.idle
                        || now.duration_since(e.created) > limits.max_age
                })
                .map(|(key, _)| *key)
                .collect();
            for key in expired {
                if let Some(entry) = table.entries.remove(&key) {
                    removed.push(self.removed(entry.id));
                }
            }
            table
                .starts
                .retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
            table
                .entries
                .iter()
                .map(|(key, e)| (*key, e.dir.clone(), e.last_seen))
                .collect()
        };
        let mut sized: Vec<([u8; 32], u64, Instant)> = snapshot
            .into_iter()
            .map(|(key, dir, seen)| (key, tree_bytes(&dir), seen))
            .collect();
        let mut total: u64 = sized.iter().map(|(_, bytes, _)| bytes).sum();
        if total > self.limits.total_bytes {
            let target = self.limits.total_bytes / 10 * 8;
            sized.sort_by_key(|(_, _, seen)| *seen);
            if let Ok(mut table) = self.table.lock() {
                for (key, bytes, _) in sized {
                    if total <= target {
                        break;
                    }
                    if let Some(entry) = table.entries.remove(&key) {
                        total = total.saturating_sub(bytes);
                        removed.push(self.removed(entry.id));
                    }
                }
            }
        }
        removed
    }

    fn removed(&self, id: Uuid) -> RemovedGuest {
        RemovedGuest {
            id,
            root: self.base.join(id.to_string()),
        }
    }

    /// Deletes the folders of removed sessions. Blocking.
    pub fn remove_folders(removed: &[RemovedGuest]) {
        for guest in removed {
            match std::fs::remove_dir_all(&guest.root) {
                Ok(()) => tracing::info!(guest_id = %guest.id, "removed guest session"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    tracing::error!(guest_id = %guest.id, error = %e, "guest folder removal failed")
                }
            }
        }
    }
}

/// Runs a sweep off the async threads and deletes what it removed.
pub async fn sweep_guests(registry: Arc<WorkspaceRegistry>) {
    let Some(guests) = registry.guests().cloned() else {
        return;
    };
    let swept = tokio::task::spawn_blocking(move || {
        let removed = guests.sweep();
        GuestSessions::remove_folders(&removed);
        removed
    })
    .await
    .unwrap_or_default();
    for guest in &swept {
        registry.forget_slots(guest.id);
    }
}

async fn discard(registry: &WorkspaceRegistry, removed: Vec<RemovedGuest>) {
    for guest in &removed {
        registry.forget_slots(guest.id);
    }
    let _ = tokio::task::spawn_blocking(move || GuestSessions::remove_folders(&removed)).await;
}

fn envelope(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            code: code.into(),
            message: message.into(),
        }),
    )
        .into_response()
}

fn guest_expired() -> Response {
    envelope(
        StatusCode::UNAUTHORIZED,
        "guest_expired",
        "This guest session has ended. Start a new one, or sign in to keep your work.",
    )
}

fn guests_disabled() -> Response {
    envelope(
        StatusCode::NOT_FOUND,
        "not_found",
        "Guest sessions are not available on this server.",
    )
}

/// What `POST /api/v1/guest` returns.
#[derive(serde::Serialize)]
struct GuestStarted {
    token: String,
    quota_bytes: u64,
    idle_secs: u64,
    max_age_secs: u64,
}

/// `POST /api/v1/guest`: starts a guest session.
pub(crate) async fn guest_start(
    State(state): State<HostedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    let Some(guests) = state.workspaces.guests().cloned() else {
        return guests_disabled();
    };
    sweep_guests(state.workspaces.clone()).await;
    match guests.create(&state.workspaces, addr.ip()) {
        Ok(token) => {
            tracing::info!(sessions = guests.count(), "guest session started");
            let limits = guests.limits();
            (
                StatusCode::CREATED,
                Json(GuestStarted {
                    token,
                    quota_bytes: limits.quota_bytes,
                    idle_secs: limits.idle.as_secs(),
                    max_age_secs: limits.max_age.as_secs(),
                }),
            )
                .into_response()
        }
        Err(GuestStartError::RateLimited) => envelope(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many guest sessions from your network. Try again later, or sign in.",
        ),
        Err(GuestStartError::Full) => envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "guest_capacity",
            "The server has too many guest sessions right now. Try again later, or sign in.",
        ),
        Err(GuestStartError::Internal(e)) => {
            tracing::error!(error = %e, "guest session start failed");
            internal_error("The guest session could not be started.").into_response()
        }
    }
}

/// `POST /api/v1/guest/ping` (body: the token): keeps a session alive.
pub(crate) async fn guest_ping(State(state): State<HostedState>, body: String) -> Response {
    let Some(guests) = state.workspaces.guests() else {
        return guests_disabled();
    };
    if guests.touch(body.trim()) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        guest_expired()
    }
}

/// `POST /api/v1/guest/end` (body: the token): deletes a session now. Sent
/// by the page as it closes (`navigator.sendBeacon`); always 204.
pub(crate) async fn guest_end(State(state): State<HostedState>, body: String) -> Response {
    if let Some(guests) = state.workspaces.guests() {
        if let Some(removed) = guests.end(body.trim()) {
            discard(&state.workspaces, vec![removed]).await;
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Splits `/g/{token}/api/v1/...?query` into the token and the inner URI.
fn split_guest_uri(uri: &Uri) -> Option<(&str, Uri)> {
    let rest = uri.path().strip_prefix("/g/")?;
    let (token, inner) = rest.split_once('/')?;
    if !inner.starts_with("api/v1/") {
        return None;
    }
    let mut target = format!("/{inner}");
    if let Some(query) = uri.query() {
        target.push('?');
        target.push_str(query);
    }
    target.parse().ok().map(|inner| (token, inner))
}

/// Handler for [`GUEST_ROUTE`]: the token, the guest's storage quota and
/// the shared analysis limits, then the guest workspace's analysis router.
pub(crate) async fn guest_dispatch(
    State(state): State<HostedState>,
    request: Request<Body>,
) -> Response {
    let Some(guests) = state.workspaces.guests().cloned() else {
        return guests_disabled();
    };
    let Some((token, inner_uri)) = split_guest_uri(request.uri()) else {
        return envelope(StatusCode::NOT_FOUND, "not_found", "Not found.");
    };
    let Some((guest_id, dir, router)) = guests.lookup(token) else {
        return guest_expired();
    };
    if grows(request.method(), inner_uri.path()) {
        // Make room first (least recently used sessions go; this one was
        // just used), then hold this session to its own quota.
        sweep_guests(state.workspaces.clone()).await;
        let usage = tokio::task::spawn_blocking(move || tree_bytes(&dir))
            .await
            .unwrap_or(u64::MAX);
        if usage >= guests.limits().quota_bytes {
            return quota_exceeded().into_response();
        }
    }
    let _permit = if is_analysis(inner_uri.path()) {
        match state.workspaces.analysis_permit(guest_id).await {
            Some(permit) => Some(permit),
            None => return server_busy(),
        }
    } else {
        None
    };
    let (mut parts, body) = request.into_parts();
    parts.uri = inner_uri;
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
    use crate::hosted::hosted_test_state;
    use crate::hosted_router;
    use crate::workspaces::DEFAULT_OPEN_WORKSPACES;
    use axum::http::header;

    fn ip(n: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, n])
    }

    fn registry_with(limits: GuestLimits) -> (Arc<WorkspaceRegistry>, PathBuf) {
        let base = tempfile::tempdir().expect("tempdir").keep();
        let guests = GuestSessions::new(base.join("guests"), limits).expect("guests");
        let registry =
            WorkspaceRegistry::new(base.join("files"), DEFAULT_OPEN_WORKSPACES).with_guests(guests);
        (Arc::new(registry), base)
    }

    #[test]
    fn startup_empties_the_guest_directory_but_refuses_a_foreign_one() {
        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("guests");
        let first = GuestSessions::new(&dir, GuestLimits::default()).expect("fresh dir");
        drop(first);
        std::fs::create_dir_all(dir.join("old-session/workspace")).expect("old");
        std::fs::write(dir.join("stray.txt"), b"x").expect("stray");
        GuestSessions::new(&dir, GuestLimits::default()).expect("own dir");
        let left: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from(MARKER)]);

        let foreign = base.path().join("user-files");
        std::fs::create_dir_all(foreign.join("users")).expect("foreign");
        assert!(GuestSessions::new(&foreign, GuestLimits::default()).is_err());
        assert!(foreign.join("users").exists(), "nothing removed");
    }

    #[test]
    fn sessions_start_end_and_are_rate_limited_and_capped() {
        let (registry, _base) = registry_with(GuestLimits {
            max_sessions: 3,
            starts_per_ip_per_hour: 2,
            ..GuestLimits::default()
        });
        let guests = registry.guests().cloned().expect("enabled");
        let a = guests.create(&registry, ip(1)).expect("first");
        let _b = guests.create(&registry, ip(1)).expect("second");
        assert!(matches!(
            guests.create(&registry, ip(1)),
            Err(GuestStartError::RateLimited)
        ));
        let _c = guests.create(&registry, ip(2)).expect("other address");
        assert!(matches!(
            guests.create(&registry, ip(3)),
            Err(GuestStartError::Full)
        ));
        assert!(guests.touch(&a));
        let (_, dir, _) = guests.lookup(&a).expect("live");
        assert!(dir.exists());
        let removed = guests.end(&a).expect("ended");
        GuestSessions::remove_folders(&[removed]);
        assert!(!dir.exists(), "folder removed");
        assert!(!guests.touch(&a), "token no longer works");
        assert!(!guests.touch("not a token!"));
        guests.create(&registry, ip(3)).expect("room again");
    }

    #[test]
    fn sweep_removes_idle_sessions_and_least_recently_used_over_the_total() {
        let (registry, _base) = registry_with(GuestLimits {
            idle: Duration::from_millis(300),
            total_bytes: 1000,
            ..GuestLimits::default()
        });
        let guests = registry.guests().cloned().expect("enabled");
        let idle = guests.create(&registry, ip(1)).expect("idle");
        std::thread::sleep(Duration::from_millis(400));
        let old = guests.create(&registry, ip(1)).expect("old");
        let new = guests.create(&registry, ip(1)).expect("new");
        let removed = guests.sweep();
        assert_eq!(removed.len(), 1, "only the idle one");
        assert!(!guests.touch(&idle));

        // 1200 bytes in all is over the 1000-byte total: the least recently
        // used session goes first, until at most 800 remain.
        let (_, old_dir, _) = guests.lookup(&old).expect("old");
        std::fs::write(old_dir.join("a.bin"), vec![0u8; 600]).expect("write");
        std::thread::sleep(Duration::from_millis(20));
        let (_, new_dir, _) = guests.lookup(&new).expect("new");
        std::fs::write(new_dir.join("b.bin"), vec![0u8; 600]).expect("write");
        let removed = guests.sweep();
        assert_eq!(removed.len(), 1);
        assert!(!guests.touch(&old), "least recently used removed");
        assert!(guests.touch(&new));
    }

    async fn call(app: &Router, method: &str, uri: &str, body: impl Into<Body>) -> Response {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "text/plain")
            .extension(ConnectInfo(SocketAddr::from(([198, 51, 100, 7], 4000))))
            .body(body.into())
            .expect("request");
        app.clone().oneshot(request).await.expect("infallible")
    }

    async fn json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json")
    }

    #[tokio::test]
    async fn guest_routes_need_no_account_and_end_with_the_page() {
        let (state, base) = hosted_test_state(crate::workspaces::tests::offline_auth_state());
        let guests = GuestSessions::new(
            base.join("guests"),
            GuestLimits {
                quota_bytes: 2048,
                ..GuestLimits::default()
            },
        )
        .expect("guests");
        let registry = WorkspaceRegistry::new(&base, DEFAULT_OPEN_WORKSPACES).with_guests(guests);
        let state = HostedState {
            workspaces: Arc::new(registry),
            ..state
        };
        let app = hosted_router(state);

        let started = call(&app, "POST", "/api/v1/guest", Body::empty()).await;
        assert_eq!(started.status(), StatusCode::CREATED);
        let body = json(started).await;
        let token = body["token"].as_str().expect("token").to_string();
        assert_eq!(body["quota_bytes"], 2048);

        // The analysis API works under /g/<token>/ with no cookie or CSRF.
        let csv = "anid,Site,fe,cr\nA1,Baca,1.5,2\nA2,Baca,2,3\n";
        let upload = call(
            &app,
            "POST",
            &format!("/g/{token}/api/v1/files?path=sources/mini.csv"),
            csv,
        )
        .await;
        assert_eq!(upload.status(), StatusCode::OK);
        let groups = call(
            &app,
            "GET",
            &format!("/g/{token}/api/v1/groups"),
            Body::empty(),
        )
        .await;
        assert_eq!(groups.status(), StatusCode::OK);

        // The guest quota refuses more data once reached.
        let big = "anid,fe\n".to_string() + &"A1,1.0\n".repeat(400);
        let over = call(
            &app,
            "POST",
            &format!("/g/{token}/api/v1/files?path=sources/big.csv"),
            big.clone(),
        )
        .await;
        assert_eq!(over.status(), StatusCode::OK, "first big upload fits");
        let refused = call(
            &app,
            "POST",
            &format!("/g/{token}/api/v1/files?path=sources/big2.csv"),
            big,
        )
        .await;
        assert_eq!(refused.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // Unknown tokens are 401 guest_expired; ping keeps a live one.
        let unknown = call(
            &app,
            "GET",
            "/g/AAAAAAAAAAAAAAAAAAAAAAAAAAAA/api/v1/groups",
            Body::empty(),
        )
        .await;
        assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(json(unknown).await["code"], "guest_expired");
        let ping = call(&app, "POST", "/api/v1/guest/ping", token.clone()).await;
        assert_eq!(ping.status(), StatusCode::NO_CONTENT);

        // Ending the session deletes it: the token stops working.
        let end = call(&app, "POST", "/api/v1/guest/end", token.clone()).await;
        assert_eq!(end.status(), StatusCode::NO_CONTENT);
        let after = call(
            &app,
            "GET",
            &format!("/g/{token}/api/v1/groups"),
            Body::empty(),
        )
        .await;
        assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
        let ping = call(&app, "POST", "/api/v1/guest/ping", token).await;
        assert_eq!(ping.status(), StatusCode::UNAUTHORIZED);
        let left: Vec<_> = std::fs::read_dir(base.join("guests"))
            .expect("dir")
            .flatten()
            .filter(|e| e.file_name() != MARKER)
            .collect();
        assert!(left.is_empty(), "guest folder removed");
    }

    #[tokio::test]
    async fn guest_routes_are_404_when_guests_are_off() {
        let (state, _base) = hosted_test_state(crate::workspaces::tests::offline_auth_state());
        let app = hosted_router(state);
        let started = call(&app, "POST", "/api/v1/guest", Body::empty()).await;
        assert_eq!(started.status(), StatusCode::NOT_FOUND);
    }
}
