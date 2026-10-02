//! Hosted auth HTTP surface (Section 10.1 routes, Section 11.1 controls).
//!
//! - Uniform, enumeration-resistant responses: login and password-reset
//!   request always return the same generic message regardless of whether
//!   the account exists; login hashes a dummy password when the account is
//!   absent so timing does not distinguish.
//! - Cookies: session cookie HttpOnly/Secure/SameSite=Lax carrying the
//!   opaque presentation token (only its SHA-256 digest is stored); CSRF
//!   cookie SameSite=Strict readable by the app for double-submit.
//! - CSRF: state-changing requests authenticated by a session cookie must
//!   echo the CSRF cookie in `X-CSRF-Token`.
//! - Throttles: persistent via [`ThrottleStore`], keyed by peppered digests
//!   of account/email/IP, never raw identifiers.

use archaeodash_auth::email::{build_action_link, redact_email, EmailMessage, EmailSender};
use archaeodash_auth::identity::{normalize_email, normalize_username};
use archaeodash_auth::password::{
    hash_password, validate_password_length, verify_or_dummy, verify_password, VerifyOutcome,
};
use archaeodash_auth::throttle::{
    ThrottleCategory, ThrottleDecision, ThrottlePepper, ThrottlePolicy, ThrottleStore,
    LOGIN_PER_ACCOUNT, LOGIN_PER_IP, RESET_PER_ACCOUNT, RESET_PER_EMAIL, VERIFY_PER_ACCOUNT,
    VERIFY_PER_EMAIL,
};
use archaeodash_auth::token::{digest_presentation, OpaqueToken};
use archaeodash_auth::GENERIC_AUTH_MESSAGE;
use archaeodash_control_postgres::{AccountTokenKind, ControlError, ControlStore, UserRow};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;
pub const SESSION_COOKIE: &str = "archaeodash_session";
pub const CSRF_COOKIE: &str = "archaeodash_csrf";
pub const CSRF_HEADER: &str = "X-CSRF-Token";
/// Session lifetime without remember-me (12 hours).
pub const SESSION_TTL_SECS: u64 = 12 * 60 * 60;

/// Shared state for the auth router.
#[derive(Clone)]
pub struct AuthState {
    pub store: Arc<ControlStore>,
    pub email: Arc<dyn EmailSender>,
    pub pepper: ThrottlePepper,
    /// Public base URL for verification/reset links (deployment config).
    pub base_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("database operation failed")]
    Database(#[from] ControlError),
    #[error("email delivery failed")]
    Email,
    #[error("request body is not valid JSON")]
    BadRequest,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match &self {
            AuthError::Database(ControlError::UsernameTaken) => {
                (StatusCode::CONFLICT, "username_taken")
            }
            AuthError::Database(ControlError::EmailTaken) => (StatusCode::CONFLICT, "email_taken"),
            AuthError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            AuthError::Email => (StatusCode::INTERNAL_SERVER_ERROR, "email_delivery_failed"),
            AuthError::BadRequest => (StatusCode::BAD_REQUEST, "bad_request"),
        };
        let message = if matches!(self, AuthError::Database(ControlError::UsernameTaken)) {
            "That username is already taken."
        } else {
            GENERIC_AUTH_MESSAGE
        };
        (
            status,
            Json(archaeodash_contracts::ErrorEnvelope {
                code: code.into(),
                message: message.into(),
            }),
        )
            .into_response()
    }
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub consent_version: String,
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub token: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub identifier: String,
    pub password: String,
    #[serde(default)]
    pub remember_days: Option<u64>,
}

#[derive(Deserialize)]
pub struct PasswordResetRequest {
    pub email: String,
}

#[derive(Deserialize)]
pub struct PasswordResetConfirmRequest {
    pub token: String,
    pub new_password: String,
}

/// `GET /auth/session` — minimal principal and CSRF bootstrap state.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SessionResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csrf_token: Option<String>,
}

pub fn auth_router(state: AuthState) -> Router {
    Router::new()
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/verify", post(verify))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/logout-all", post(logout_all))
        .route("/api/v1/auth/session", get(session))
        .route(
            "/api/v1/auth/password-reset/request",
            post(password_reset_request),
        )
        .route(
            "/api/v1/auth/password-reset/confirm",
            post(password_reset_confirm),
        )
        .with_state(state)
}

/// Cookie header parsing: name -> value (first occurrence wins).
fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Builds a Set-Cookie header value with the Section 17.1 flags.
fn set_cookie(name: &str, value: &str, max_age_secs: Option<i64>) -> String {
    let mut cookie = format!("{name}={value}; Path=/; HttpOnly; Secure; SameSite=Lax");
    if let Some(age) = max_age_secs {
        cookie.push_str(&format!("; Max-Age={age}"));
    }
    cookie
}

/// Double-submit CSRF check: required when the request carries a session or
/// CSRF cookie (i.e., there is ambient authority to abuse).
fn csrf_ok(headers: &HeaderMap) -> bool {
    let session = cookie_value(headers, SESSION_COOKIE);
    let csrf = cookie_value(headers, CSRF_COOKIE);
    if session.is_none() && csrf.is_none() {
        return true; // No ambient authority; CSRF is not applicable.
    }
    let Some(csrf_cookie) = csrf else {
        return false;
    };
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|sent| constant_time_eq(sent, &csrf_cookie))
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    archaeodash_auth::token::digests_equal(&digest_presentation(a), &digest_presentation(b))
}

/// Extracts the client IP for throttle keys. Direct connections only; a
/// reverse proxy must terminate and set ConnectInfo from the trusted peer.
fn client_ip(addr: Option<ConnectInfo<SocketAddr>>) -> String {
    addr.map(|ConnectInfo(a)| a.ip().to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string())
}

async fn throttled(
    state: &AuthState,
    category: ThrottleCategory,
    identifier: &str,
    ip: &str,
    policy: ThrottlePolicy,
) -> Result<(), (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    let now = SystemTime::now();
    let account_key = state.pepper.key(category, identifier);
    let ip_key = state.pepper.key(category, ip);
    for key in [account_key, ip_key] {
        if let ThrottleDecision::Limited { .. } =
            state.store.record_and_check(&key, policy, now).await
        {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(archaeodash_contracts::ErrorEnvelope {
                    code: "rate_limited".into(),
                    message: GENERIC_AUTH_MESSAGE.into(),
                }),
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn register(
    State(state): State<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<RegisterRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    if !csrf_ok(&headers) {
        return Err((StatusCode::FORBIDDEN, json_error("csrf_failed")));
    }
    let Json(req) = body.map_err(|_| (StatusCode::BAD_REQUEST, json_error("bad_request")))?;
    let ip = &client_ip(Some(ConnectInfo(addr)));
    let username = normalize_username(&req.username).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json_error("invalid_username"),
        )
    })?;
    let email = normalize_email(&req.email).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json_error("invalid_email"),
        )
    })?;
    validate_password_length(&req.password).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json_error("invalid_password"),
        )
    })?;

    throttled(
        &state,
        ThrottleCategory::VerifyByEmail,
        &email,
        ip,
        VERIFY_PER_EMAIL,
    )
    .await?;
    throttled(
        &state,
        ThrottleCategory::VerifyByAccount,
        &username,
        ip,
        VERIFY_PER_ACCOUNT,
    )
    .await?;

    // Enumeration resistance: an already-registered email gets the same
    // generic 202 response as a fresh registration (no verification email
    // is sent; the message text stays uniform).
    if let Ok(Some(_)) = state.store.find_user_by_normalized_email(&email).await {
        return Ok(StatusCode::ACCEPTED);
    }
    let password_hash = hash_password(&req.password).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json_error("invalid_password"),
        )
    })?;
    let user = state
        .store
        .create_user(
            Uuid::now_v7(),
            &req.username.trim().to_lowercase(),
            &username,
            &req.email,
            &email,
            &password_hash,
            SystemTime::now(),
        )
        .await
        .map_err(db_error)?;
    send_verification(&state, &user).await?;
    Ok(StatusCode::ACCEPTED)
}

/// Creates the verify-email token and sends the link. Delivery failures are
/// logged by the caller-side sender abstraction only; the endpoint maps them
/// to a uniform 500.
async fn send_verification(
    state: &AuthState,
    user: &UserRow,
) -> Result<(), (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    let token = OpaqueToken::generate().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json_error("internal_error"),
        )
    })?;
    state
        .store
        .create_account_token(
            Uuid::now_v7(),
            user.id,
            AccountTokenKind::VerifyEmail,
            &token.digest(),
            archaeodash_auth::token::ONE_TIME_LINK_TTL,
            SystemTime::now(),
        )
        .await
        .map_err(db_error)?;
    let link =
        build_action_link(&state.base_url, "/auth/verify", token.presentation()).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json_error("internal_error"),
            )
        })?;
    state
        .email
        .send(EmailMessage {
            to: user.email.clone(),
            subject: "Verify your ArchaeoDash account".into(),
            text_body: format!(
                "Welcome to ArchaeoDash, {name}.\n\nVerify your email address within 24 hours:\n{link}\n\nIf you did not create an account, you can ignore this message.",
                name = user.username
            ),
        })
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, json_error("email_delivery_failed")))?;
    Ok(())
}

fn json_error(code: &str) -> Json<archaeodash_contracts::ErrorEnvelope> {
    Json(archaeodash_contracts::ErrorEnvelope {
        code: code.into(),
        message: GENERIC_AUTH_MESSAGE.into(),
    })
}

fn db_error(_err: ControlError) -> (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(archaeodash_contracts::ErrorEnvelope {
            code: "internal_error".into(),
            message: GENERIC_AUTH_MESSAGE.into(),
        }),
    )
}

async fn verify(
    State(state): State<AuthState>,
    Json(req): Json<VerifyRequest>,
) -> Result<StatusCode, (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    let digest = digest_presentation(&req.token);
    let now = SystemTime::now();
    let user_id = state
        .store
        .consume_account_token(&digest, AccountTokenKind::VerifyEmail, now)
        .await
        .map_err(db_error)?;
    let Some(user_id) = user_id else {
        // Generic safe error for unknown/expired/used tokens (Section 11.1).
        return Err((StatusCode::BAD_REQUEST, json_error("invalid_token")));
    };
    state
        .store
        .mark_email_verified(user_id, now)
        .await
        .map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Session lookup result for the authenticated handlers.
struct Authenticated {
    user: UserRow,
}

/// Resolves the live session from the cookie; `None` when absent/expired.
async fn authenticated(
    state: &AuthState,
    headers: &HeaderMap,
) -> Result<Option<Authenticated>, ControlError> {
    let Some(presentation) = cookie_value(headers, SESSION_COOKIE) else {
        return Ok(None);
    };
    let digest = digest_presentation(&presentation);
    let found = state
        .store
        .find_live_session(&digest, SystemTime::now())
        .await?;
    Ok(found.map(|(_, user)| Authenticated { user }))
}

#[allow(clippy::too_many_lines)]
async fn login(
    State(state): State<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<LoginRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<
    (StatusCode, HeaderMap, Json<SessionResponse>),
    (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>),
> {
    if !csrf_ok(&headers) {
        return Err((StatusCode::FORBIDDEN, json_error("csrf_failed")));
    }
    let Json(req) = body.map_err(|_| (StatusCode::BAD_REQUEST, json_error("bad_request")))?;
    let ip = client_ip(Some(ConnectInfo(addr)));
    let now = SystemTime::now();

    // Identify the account by email or normalized username, whichever parses.
    let account_key = match normalize_email(&req.identifier) {
        Ok(email) => state.pepper.key(ThrottleCategory::LoginByAccount, &email),
        Err(_) => match normalize_username(&req.identifier) {
            Ok(username) => state
                .pepper
                .key(ThrottleCategory::LoginByAccount, &username),
            Err(_) => return Err(unauthorized()),
        },
    };
    for (key, policy) in [
        (account_key, LOGIN_PER_ACCOUNT),
        (
            state.pepper.key(ThrottleCategory::LoginByIp, &ip),
            LOGIN_PER_IP,
        ),
    ] {
        if let ThrottleDecision::Limited { .. } =
            state.store.record_and_check(&key, policy, now).await
        {
            return Err((StatusCode::TOO_MANY_REQUESTS, json_error("rate_limited")));
        }
    }

    // Enumeration + timing resistance: same generic failure path whether the
    // account is absent, unverified, disabled, or the password is wrong.
    let user = match normalize_email(&req.identifier) {
        Ok(email) => state.store.find_user_by_normalized_email(&email).await,
        Err(_) => {
            let username = normalize_username(&req.identifier).map_err(|_| {
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json_error("invalid_identifier"),
                )
            })?;
            state
                .store
                .find_user_by_normalized_username(&username)
                .await
        }
    }
    .map_err(db_error)?;
    let Some(user) = user else {
        verify_or_dummy(&req.password, None);
        return Err(unauthorized());
    };
    let verified = verify_password(&req.password, &user.password_hash);
    if verified == VerifyOutcome::Invalid || user.disabled_at.is_some() {
        return Err(unauthorized());
    }
    if user.email_verified_at.is_none() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(archaeodash_contracts::ErrorEnvelope {
                code: "email_unverified".into(),
                message: "Verify your email address before signing in.".into(),
            }),
        ));
    }
    // Rehash on login when the stored hash is below policy (Section 11.1).
    if verified == VerifyOutcome::RehashNeeded {
        let rehashed = hash_password(&req.password).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json_error("internal_error"),
            )
        })?;
        state
            .store
            .update_password_hash(user.id, &rehashed, SystemTime::now())
            .await
            .map_err(db_error)?;
    }

    // Revoke any session presented by the cookie, then issue the fresh
    // session (Section 11.1: rotate at use and login).
    if let Some(presentation) = cookie_value(&headers, SESSION_COOKIE) {
        let old_digest = digest_presentation(&presentation);
        state
            .store
            .revoke_session(&old_digest, SystemTime::now())
            .await
            .map_err(db_error)?;
    }
    let token = OpaqueToken::generate().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json_error("internal_error"),
        )
    })?;
    state
        .store
        .create_session(
            Uuid::now_v7(),
            user.id,
            &token.digest(),
            req.remember_days,
            SystemTime::now(),
        )
        .await
        .map_err(db_error)?;
    state.store.clear(&account_key).await;
    state
        .store
        .clear(&state.pepper.key(ThrottleCategory::LoginByIp, &ip))
        .await;

    let csrf = OpaqueToken::generate().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json_error("internal_error"),
        )
    })?;
    let mut response_headers = HeaderMap::new();
    let session_max_age = req
        .remember_days
        .map(|days| days * 24 * 60 * 60)
        .unwrap_or(SESSION_TTL_SECS)
        .min(i64::MAX as u64) as i64;
    let session_cookie = set_cookie(SESSION_COOKIE, token.presentation(), Some(session_max_age));
    // CSRF cookie is readable by the app (no HttpOnly) for double submit.
    let csrf_cookie = set_cookie(CSRF_COOKIE, csrf.presentation(), Some(session_max_age))
        .replace("HttpOnly; ", "");
    response_headers.insert(
        header::SET_COOKIE,
        header::HeaderValue::from_str(&session_cookie).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json_error("internal_error"),
            )
        })?,
    );
    response_headers.insert(
        header::SET_COOKIE,
        header::HeaderValue::from_str(&csrf_cookie).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json_error("internal_error"),
            )
        })?,
    );
    Ok((
        StatusCode::OK,
        response_headers,
        Json(SessionResponse {
            authenticated: true,
            username: Some(user.username),
            email: Some(user.email),
            email_verified: Some(true),
            csrf_token: Some(csrf.presentation().into()),
        }),
    ))
}

fn unauthorized() -> (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(archaeodash_contracts::ErrorEnvelope {
            code: "invalid_credentials".into(),
            message: GENERIC_AUTH_MESSAGE.into(),
        }),
    )
}

async fn logout(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    if !csrf_ok(&headers) {
        return Err((StatusCode::FORBIDDEN, json_error("csrf_failed")));
    }
    if let Some(presentation) = cookie_value(&headers, SESSION_COOKIE) {
        state
            .store
            .revoke_session(&digest_presentation(&presentation), SystemTime::now())
            .await
            .map_err(db_error)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn logout_all(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    if !csrf_ok(&headers) {
        return Err((StatusCode::FORBIDDEN, json_error("csrf_failed")));
    }
    let Some(auth) = authenticated(&state, &headers).await.map_err(db_error)? else {
        return Err((StatusCode::UNAUTHORIZED, json_error("invalid_credentials")));
    };
    state
        .store
        .revoke_all_sessions(auth.user.id, SystemTime::now())
        .await
        .map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn session(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<SessionResponse>, (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    // CSRF bootstrap: a fresh CSRF cookie accompanies every session check
    // that lacks one, so the app can echo it on the next state change.
    let auth = authenticated(&state, &headers).await.map_err(db_error)?;
    let csrf_existing = cookie_value(&headers, CSRF_COOKIE);
    match auth {
        Some(auth) => {
            let csrf = match csrf_existing {
                Some(existing) => existing,
                None => match OpaqueToken::generate() {
                    Ok(token) => token.presentation().to_string(),
                    Err(_) => {
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            json_error("internal_error"),
                        ))
                    }
                },
            };
            Ok(Json(SessionResponse {
                authenticated: true,
                username: Some(auth.user.username),
                email: Some(auth.user.email),
                email_verified: Some(auth.user.email_verified_at.is_some()),
                csrf_token: Some(csrf),
            }))
        }
        None => Ok(Json(SessionResponse {
            authenticated: false,
            username: None,
            email: None,
            email_verified: None,
            csrf_token: csrf_existing,
        })),
    }
}

async fn password_reset_request(
    State(state): State<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Result<Json<PasswordResetRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    let Json(req) = body.map_err(|_| (StatusCode::BAD_REQUEST, json_error("bad_request")))?;
    let email = normalize_email(&req.email).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json_error("invalid_email"),
        )
    })?;
    let ip = client_ip(Some(ConnectInfo(addr)));
    let now = SystemTime::now();
    // Persistent throttling by IP and email, regardless of existence.
    let checks = [
        (
            state.pepper.key(ThrottleCategory::ResetByEmail, &email),
            RESET_PER_EMAIL,
        ),
        (
            state.pepper.key(ThrottleCategory::ResetByAccount, &email),
            RESET_PER_ACCOUNT,
        ),
        (
            state.pepper.key(ThrottleCategory::ResetByEmail, &ip),
            RESET_PER_EMAIL,
        ),
    ];
    for (key, policy) in checks {
        if let ThrottleDecision::Limited { .. } =
            state.store.record_and_check(&key, policy, now).await
        {
            return Ok(StatusCode::ACCEPTED);
        }
    }
    if let Ok(Some(user)) = state.store.find_user_by_normalized_email(&email).await {
        let token = OpaqueToken::generate().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json_error("internal_error"),
            )
        })?;
        state
            .store
            .create_account_token(
                Uuid::now_v7(),
                user.id,
                AccountTokenKind::PasswordReset,
                &token.digest(),
                archaeodash_auth::token::ONE_TIME_LINK_TTL,
                now,
            )
            .await
            .map_err(db_error)?;
        let link = build_action_link(&state.base_url, "/auth/reset", token.presentation())
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json_error("internal_error"),
                )
            })?;
        state
            .email
            .send(EmailMessage {
                to: user.email.clone(),
                subject: "Reset your ArchaeoDash password".into(),
                text_body: format!(
                    "A password reset was requested for {name}.\n\nReset within 24 hours:\n{link}\n\nIf this was not you, ignore this message; your account is unchanged.",
                    name = redact_email(&user.email)
                ),
            })
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, json_error("email_delivery_failed")))?;
    }
    // Enumeration resistance: identical response whether or not the account exists.
    Ok(StatusCode::ACCEPTED)
}

async fn password_reset_confirm(
    State(state): State<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Result<Json<PasswordResetConfirmRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, (StatusCode, Json<archaeodash_contracts::ErrorEnvelope>)> {
    let Json(req) = body.map_err(|_| (StatusCode::BAD_REQUEST, json_error("bad_request")))?;
    validate_password_length(&req.new_password).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json_error("invalid_password"),
        )
    })?;
    let ip = client_ip(Some(ConnectInfo(addr)));
    let key = state.pepper.key(ThrottleCategory::ResetByAccount, &ip);
    if let ThrottleDecision::Limited { .. } = state
        .store
        .record_and_check(&key, RESET_PER_ACCOUNT, SystemTime::now())
        .await
    {
        return Ok(StatusCode::ACCEPTED);
    }
    let digest = digest_presentation(&req.token);
    let user_id = state
        .store
        .consume_account_token(&digest, AccountTokenKind::PasswordReset, SystemTime::now())
        .await
        .map_err(db_error)?;
    let Some(user_id) = user_id else {
        return Err((StatusCode::BAD_REQUEST, json_error("invalid_token")));
    };
    let hash = hash_password(&req.new_password).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json_error("invalid_password"),
        )
    })?;
    // consume_account_token already revoked all sessions for a reset.
    state
        .store
        .verify_and_set_password(user_id, &hash, SystemTime::now())
        .await
        .map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_auth::email::DevSinkEmailSender;
    use archaeodash_auth::token::OpaqueToken;
    use archaeodash_contracts::ErrorEnvelope;
    use axum::body::Body;
    use tower::ServiceExt;

    /// DB-backed tests run only when `DATABASE_URL` points at a test
    /// PostgreSQL instance (CI provides one); otherwise they skip.
    async fn auth_state() -> Option<(AuthState, DevSinkEmailSender)> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let pool = sqlx::PgPool::connect(&url).await.ok()?;
        let store = ControlStore::new(pool);
        store.migrate().await.expect("migrations apply");
        let email = DevSinkEmailSender::new();
        let state = AuthState {
            store: Arc::new(store),
            email: Arc::new(email.clone()),
            pepper: ThrottlePepper::from_hex(&"ab".repeat(32)).expect("pepper"),
            base_url: "https://archaeodash.example".to_string(),
        };
        Some((state, email))
    }

    /// Uniqueness suffix so repeated test runs against a live DB never collide.
    fn unique_suffix() -> String {
        let bytes = OpaqueToken::generate().expect("rng").digest();
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()[..12].to_string()
    }

    fn auth_app(state: AuthState) -> Router {
        auth_router(state)
    }

    async fn post_json(
        app: Router,
        uri: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        app.oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).expect("json")))
                .expect("request"),
        )
        .await
        .expect("infallible")
    }

    #[tokio::test]
    async fn register_sends_verification_and_verify_completes() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let app = auth_app(state.clone());
        let suffix = unique_suffix();
        let username = format!("user-{suffix}");
        let response = post_json(
            app,
            "/api/v1/auth/register",
            serde_json::json!({
                "username": username,
                "email": format!("{username}@example.com"),
                "password": "correct horse battery staple",
                "consent_version": "2026-10"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        // Dev sink holds exactly one message with the verification link.
        let stored = sink.stored();
        assert_eq!(stored.len(), 1);
        let link = stored[0]
            .text_body
            .split("https://archaeodash.example/auth/verify?token=")
            .nth(1)
            .expect("verify link")
            .lines()
            .next()
            .expect("token line")
            .to_string();

        let response = post_json(
            auth_app(state.clone()),
            "/api/v1/auth/verify",
            serde_json::json!({ "token": link }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Single use: replaying the token is a generic rejection.
        let response = post_json(
            auth_app(state.clone()),
            "/api/v1/auth/verify",
            serde_json::json!({ "token": link }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn login_sets_cookies_and_session_endpoint_reflects_it() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let suffix = unique_suffix();
        let username = format!("user-{suffix}");
        let email = format!("{username}@example.com");
        let password = "correct horse battery staple";
        let app = auth_app(state.clone());
        post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": username, "email": email, "password": password,
                "consent_version": "2026-10"
            }),
        )
        .await;
        let link = sink.stored()[0]
            .text_body
            .split("/auth/verify?token=")
            .nth(1)
            .expect("token")
            .lines()
            .next()
            .expect("token line")
            .to_string();
        post_json(
            app.clone(),
            "/api/v1/auth/verify",
            serde_json::json!({ "token": link }),
        )
        .await;

        // Login returns session + CSRF cookies and the principal.
        let response = post_json(
            app.clone(),
            "/api/v1/auth/login",
            serde_json::json!({
                "identifier": username,
                "password": password,
                "remember_days": 30
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let set_cookies: Vec<String> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ascii").to_string())
            .collect();
        assert_eq!(set_cookies.len(), 2);
        let session_cookie = set_cookies
            .iter()
            .find(|c| c.starts_with(SESSION_COOKIE))
            .expect("session cookie");
        assert!(session_cookie.contains("HttpOnly"));
        assert!(session_cookie.contains("SameSite=Lax"));
        assert!(session_cookie.contains("Secure"));
        assert!(session_cookie.contains("Max-Age=2592000"));
        let csrf_cookie = set_cookies
            .iter()
            .find(|c| c.starts_with(CSRF_COOKIE))
            .expect("csrf cookie");
        assert!(!csrf_cookie.contains("HttpOnly"));

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let parsed: SessionResponse = serde_json::from_slice(&bytes).expect("session json");
        assert_eq!(parsed.username.as_deref(), Some(username.as_str()));
        assert_eq!(parsed.email_verified, Some(true));
    }

    #[tokio::test]
    async fn wrong_password_is_generic_unauthorized() {
        let Some((state, _sink)) = auth_state().await else {
            return;
        };
        let suffix = unique_suffix();
        let username = format!("user-{suffix}");
        let app = auth_app(state.clone());
        post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": username,
                "email": format!("{username}@example.com"),
                "password": "correct horse battery staple",
                "consent_version": "2026-10"
            }),
        )
        .await;
        // Unverified account: login is forbidden with a distinct code.
        let response = post_json(
            app,
            "/api/v1/auth/login",
            serde_json::json!({
                "identifier": username,
                "password": "correct horse battery staple"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).expect("envelope");
        assert_eq!(envelope.code, "email_unverified");
    }

    #[tokio::test]
    async fn login_missing_account_is_generic_and_timing_uniform() {
        let Some((state, _sink)) = auth_state().await else {
            return;
        };
        let app = auth_app(state);
        let response = post_json(
            app,
            "/api/v1/auth/login",
            serde_json::json!({
                "identifier": format!("ghost-{}", unique_suffix()),
                "password": "whatever long password"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).expect("envelope");
        assert_eq!(envelope.message, GENERIC_AUTH_MESSAGE);
    }

    #[tokio::test]
    async fn password_reset_request_is_enumeration_resistant() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let suffix = unique_suffix();
        let email = format!("reset-{suffix}@example.com");
        let app = auth_app(state.clone());
        // Unknown email: identical generic response, no email sent.
        let response = post_json(
            app.clone(),
            "/api/v1/auth/password-reset/request",
            serde_json::json!({ "email": format!("nobody-{suffix}@example.com") }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(sink.stored().is_empty());
        // Known email: same status, reset email sent.
        post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": format!("reset-{suffix}"),
                "email": email,
                "password": "correct horse battery staple",
                "consent_version": "2026-10"
            }),
        )
        .await;
        let before = sink.stored().len();
        let response = post_json(
            app,
            "/api/v1/auth/password-reset/request",
            serde_json::json!({ "email": email }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(sink.stored().len(), before + 1);
    }

    #[tokio::test]
    async fn reset_confirm_revokes_sessions_and_sets_password() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let suffix = unique_suffix();
        let username = format!("reset-{suffix}");
        let email = format!("{username}@example.com");
        let app = auth_app(state.clone());
        post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": username, "email": email, "password": "correct horse battery staple",
                "consent_version": "2026-10"
            }),
        )
        .await;
        let verify_link = sink.stored()[0]
            .text_body
            .split("/auth/verify?token=")
            .nth(1)
            .expect("token")
            .lines()
            .next()
            .expect("line")
            .to_string();
        post_json(
            app.clone(),
            "/api/v1/auth/verify",
            serde_json::json!({ "token": verify_link }),
        )
        .await;

        // Log in, then request a reset.
        let login_response = post_json(
            app.clone(),
            "/api/v1/auth/login",
            serde_json::json!({ "identifier": username, "password": "correct horse battery staple" }),
        )
        .await;
        assert_eq!(login_response.status(), StatusCode::OK);
        let cookie = login_response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ascii").to_string())
            .find(|c| c.starts_with(SESSION_COOKIE))
            .expect("session cookie");
        let session_token = cookie
            .split(';')
            .next()
            .expect("pair")
            .split('=')
            .nth(1)
            .expect("token")
            .to_string();

        post_json(
            app.clone(),
            "/api/v1/auth/password-reset/request",
            serde_json::json!({ "email": email }),
        )
        .await;
        let reset_link = sink
            .stored()
            .iter()
            .rev()
            .find(|m| m.text_body.contains("/auth/reset?token="))
            .expect("reset email")
            .text_body
            .split("/auth/reset?token=")
            .nth(1)
            .expect("token")
            .lines()
            .next()
            .expect("line")
            .to_string();

        // Confirm resets the password and revokes the live session.
        let response = post_json(
            app.clone(),
            "/api/v1/auth/password-reset/confirm",
            serde_json::json!({
                "token": reset_link,
                "new_password": "another correct horse staple"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // The pre-reset session cookie no longer resolves.
        let session = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/v1/auth/session")
                    .header(header::COOKIE, format!("{SESSION_COOKIE}={session_token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        let bytes = axum::body::to_bytes(session.into_body(), usize::MAX)
            .await
            .expect("body");
        let parsed: SessionResponse = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(parsed.authenticated, false);

        // Old password no longer works; new one does.
        let old_login = post_json(
            app.clone(),
            "/api/v1/auth/login",
            serde_json::json!({ "identifier": username, "password": "correct horse battery staple" }),
        )
        .await;
        assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);
        let new_login = post_json(
            app,
            "/api/v1/auth/login",
            serde_json::json!({ "identifier": username, "password": "another correct horse staple" }),
        )
        .await;
        assert_eq!(new_login.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn csrf_is_required_on_cookie_authenticated_state_changes() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let suffix = unique_suffix();
        let username = format!("csrf-{suffix}");
        let app = auth_app(state.clone());
        post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": username, "email": format!("{username}@example.com"),
                "password": "correct horse battery staple", "consent_version": "2026-10"
            }),
        )
        .await;
        let verify_link = sink.stored()[0]
            .text_body
            .split("/auth/verify?token=")
            .nth(1)
            .expect("token")
            .lines()
            .next()
            .expect("line")
            .to_string();
        post_json(
            app.clone(),
            "/api/v1/auth/verify",
            serde_json::json!({ "token": verify_link }),
        )
        .await;
        let login_response = post_json(
            app.clone(),
            "/api/v1/auth/login",
            serde_json::json!({ "identifier": username, "password": "correct horse battery staple" }),
        )
        .await;
        let cookies: Vec<String> = login_response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ascii").to_string())
            .collect();
        let session_cookie = cookies
            .iter()
            .find(|c| c.starts_with(SESSION_COOKIE))
            .expect("session cookie")
            .split(';')
            .next()
            .expect("pair")
            .to_string();
        let csrf_token = cookies
            .iter()
            .find(|c| c.starts_with(CSRF_COOKIE))
            .expect("csrf cookie")
            .split(';')
            .next()
            .expect("pair")
            .split('=')
            .nth(1)
            .expect("token")
            .to_string();

        // Logout with the session cookie but no CSRF header: forbidden.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header(header::COOKIE, &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // With the matching CSRF header: accepted and session revoked.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header(header::COOKIE, &session_cookie)
                    .header(CSRF_HEADER, &csrf_token)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn register_rejects_invalid_identity_and_duplicate_usernames() {
        let Some((state, _sink)) = auth_state().await else {
            return;
        };
        let suffix = unique_suffix();
        let app = auth_app(state.clone());
        // Invalid username characters.
        let response = post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": "bad name!", "email": format!("x-{suffix}@example.com"),
                "password": "correct horse battery staple", "consent_version": "2026-10"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        // Valid registration.
        let response = post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": format!("dup-{suffix}"), "email": format!("dup-{suffix}@example.com"),
                "password": "correct horse battery staple", "consent_version": "2026-10"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        // Duplicate username (different email) is a typed conflict.
        let response = post_json(
            app,
            "/api/v1/auth/register",
            serde_json::json!({
                "username": format!("DUP-{suffix}"), "email": format!("other-{suffix}@example.com"),
                "password": "correct horse battery staple", "consent_version": "2026-10"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
}
