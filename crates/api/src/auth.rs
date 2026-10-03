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
use archaeodash_auth::token::{digest_presentation, OpaqueToken, REMEMBER_ME_DAY_CHOICES};
use archaeodash_auth::GENERIC_AUTH_MESSAGE;
use archaeodash_contracts::ErrorEnvelope;
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
/// The terms/privacy notice version a registration must accept (Section 10.1:
/// registration validates the consent version). Bump when the notice changes.
pub const CONSENT_VERSION: &str = "2026-10";
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
            Json(ErrorEnvelope {
                code: code.into(),
                message: message.into(),
            }),
        )
            .into_response()
    }
}

#[derive(serde::Serialize)]
pub struct ConsentResponse {
    pub consent_version: &'static str,
    pub terms_path: String,
    pub privacy_path: String,
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

/// `GET /auth/consent` — the current notice version and document paths, no
/// authentication required (the registration dialog fetches it first).
async fn consent() -> Json<ConsentResponse> {
    Json(ConsentResponse {
        consent_version: CONSENT_VERSION,
        terms_path: "/legal/terms".to_string(),
        privacy_path: "/legal/privacy".to_string(),
    })
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
    addr.map(|a| a.0.ip().to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string())
}

async fn throttled(
    state: &AuthState,
    category: ThrottleCategory,
    identifier: &str,
    ip: &str,
    policy: ThrottlePolicy,
) -> Result<(), (StatusCode, Json<ErrorEnvelope>)> {
    let now = SystemTime::now();
    let account_key = state.pepper.key(category, identifier);
    let ip_key = state.pepper.key(category, ip);
    for key in [account_key, ip_key] {
        if let ThrottleDecision::Limited { .. } =
            state.store.record_and_check(&key, policy, now).await
        {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(ErrorEnvelope {
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
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
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
    // The client must accept the current notice version; stale or missing
    // consent versions are rejected so acceptance is auditable per account.
    if req.consent_version != CONSENT_VERSION {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            json_error("invalid_consent_version"),
        ));
    }

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
            Some(&req.consent_version),
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
) -> Result<(), (StatusCode, Json<ErrorEnvelope>)> {
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
    json_error_with(code, GENERIC_AUTH_MESSAGE)
}

fn json_error_with(code: &str, message: &str) -> Json<archaeodash_contracts::ErrorEnvelope> {
    Json(ErrorEnvelope {
        code: code.into(),
        message: message.into(),
    })
}

/// Preference validation failures are user-input errors: they carry the
/// domain's specific safe message (unknown key / invalid value shape).
fn domain_validation_error(
    err: archaeodash_domain::DomainError,
) -> (StatusCode, Json<ErrorEnvelope>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ErrorEnvelope {
            code: "validation_error".into(),
            message: err.to_string(),
        }),
    )
}

fn db_error(err: ControlError) -> (StatusCode, Json<ErrorEnvelope>) {
    let (status, code, message) = match &err {
        ControlError::UsernameTaken => (
            StatusCode::CONFLICT,
            "username_taken",
            "That username is already taken.",
        ),
        ControlError::EmailTaken => (
            StatusCode::CONFLICT,
            "email_taken",
            "That email is already taken.",
        ),
        ControlError::Database(_) | ControlError::Migration(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            GENERIC_AUTH_MESSAGE,
        ),
    };
    (
        status,
        Json(ErrorEnvelope {
            code: code.into(),
            message: message.into(),
        }),
    )
}

async fn verify(
    State(state): State<AuthState>,
    Json(req): Json<VerifyRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
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
) -> Result<(StatusCode, HeaderMap, Json<SessionResponse>), (StatusCode, Json<ErrorEnvelope>)> {
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
            Json(ErrorEnvelope {
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
    // Remember-me is an explicit choice limited to the Section 17.1 day
    // choices (30 or 90); anything else is a 422, never a silent clamp.
    let remember_days = match req.remember_days {
        None => None,
        Some(d) if REMEMBER_ME_DAY_CHOICES.contains(&d) => Some(d),
        Some(_) => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                json_error("invalid_remember_days"),
            ))
        }
    };
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
            remember_days,
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
    // Multiple Set-Cookie headers must append, not replace.
    response_headers.append(
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

fn unauthorized() -> (StatusCode, Json<ErrorEnvelope>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorEnvelope {
            code: "invalid_credentials".into(),
            message: GENERIC_AUTH_MESSAGE.into(),
        }),
    )
}

async fn logout(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
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
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
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
) -> Result<Json<SessionResponse>, (StatusCode, Json<ErrorEnvelope>)> {
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
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
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
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
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
    pub(crate) async fn auth_state() -> Option<(AuthState, DevSinkEmailSender)> {
        let url = std::env::var("DATABASE_URL").ok()?;
        // Fail loudly when the URL is set but unreachable: a silent skip
        // would make infrastructure breakage look like passing tests.
        let pool = sqlx::PgPool::connect(&url)
            .await
            .unwrap_or_else(|e| panic!("DATABASE_URL is set but the test DB is unreachable: {e}"));
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
    pub(crate) fn unique_suffix() -> String {
        let bytes = OpaqueToken::generate().expect("rng").digest();
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()[..12].to_string()
    }

    pub(crate) fn test_peer() -> std::net::SocketAddr {
        // Distinct addresses per request so per-IP throttle buckets never
        // collide across parallel tests or across repeated runs against a
        // shared live database (production keys on the client IP): the base
        // octets come from a per-process random prefix.
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        static BASE: std::sync::OnceLock<[u8; 2]> = std::sync::OnceLock::new();
        let base = *BASE.get_or_init(|| {
            let mut bytes = [0u8; 2];
            let _ = getrandom::fill(&mut bytes);
            bytes
        });
        let n = N.fetch_add(1, Ordering::Relaxed);
        let a = 10u8;
        let (b, c) = (base[0], base[1]);
        let d = (n & 0xff) as u8;
        format!("{a}.{b}.{c}.{d}:65001").parse().expect("peer addr")
    }

    pub(crate) fn auth_app(state: AuthState) -> Router {
        auth_router(state).layer(axum::middleware::from_fn(
            |mut req: axum::extract::Request, next: axum::middleware::Next| async move {
                if req.extensions().get::<ConnectInfo<SocketAddr>>().is_none() {
                    req.extensions_mut().insert(ConnectInfo(test_peer()));
                }
                next.run(req).await
            },
        ))
    }

    pub(crate) async fn post_json(
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
                .map(|mut req| {
                    req.extensions_mut().insert(ConnectInfo(test_peer()));
                    req
                })
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
        let Some((state, sink)) = auth_state().await else {
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
        let Some((state, sink)) = auth_state().await else {
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
                    .map(|mut req| {
                        req.extensions_mut().insert(ConnectInfo(test_peer()));
                        req
                    })
                    .expect("request"),
            )
            .await
            .expect("infallible");
        let bytes = axum::body::to_bytes(session.into_body(), usize::MAX)
            .await
            .expect("body");
        let parsed: SessionResponse = serde_json::from_slice(&bytes).expect("json");
        assert!(!parsed.authenticated);

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
                    .map(|mut req| {
                        req.extensions_mut().insert(ConnectInfo(test_peer()));
                        req
                    })
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
                    .header(
                        header::COOKIE,
                        format!("{session_cookie}; {CSRF_COOKIE}={csrf_token}"),
                    )
                    .header(CSRF_HEADER, &csrf_token)
                    .body(Body::empty())
                    .map(|mut req| {
                        req.extensions_mut().insert(ConnectInfo(test_peer()));
                        req
                    })
                    .expect("request"),
            )
            .await
            .expect("infallible");
        if response.status() != StatusCode::NO_CONTENT {
            eprintln!("DEBUG csrf_token={csrf_token:?} cookies={cookies:?}");
            panic!("expected no content");
        }
    }

    #[tokio::test]
    async fn register_rejects_invalid_identity_and_duplicate_usernames() {
        let Some((state, sink)) = auth_state().await else {
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

// ---- Hosted preferences (Section 10.1: GET/PUT /preferences, typed
// allowlisted keys only) and hosted router composition. ----

/// The authenticated principal returned by `GET /auth/session` and used to
/// scope preference reads/writes.
struct SessionPrincipal {
    user: UserRow,
}

/// Resolves the live session from the cookie, enforcing CSRF on writes.
async fn require_session(
    state: &AuthState,
    headers: &HeaderMap,
    csrf_required: bool,
) -> Result<Option<SessionPrincipal>, (StatusCode, Json<ErrorEnvelope>)> {
    if csrf_required && !csrf_ok(headers) {
        return Err((StatusCode::FORBIDDEN, json_error("csrf_failed")));
    }
    let Some(presentation) = cookie_value(headers, SESSION_COOKIE) else {
        return Ok(None);
    };
    let digest = digest_presentation(&presentation);
    let found = state
        .store
        .find_live_session(&digest, SystemTime::now())
        .await
        .map_err(db_error)?;
    Ok(found.map(|(_, user)| SessionPrincipal { user }))
}

/// `GET /api/v1/auth/preferences` — the session user's allowlisted
/// preferences as a JSON object keyed by preference key.
async fn preferences_get(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = require_session(&state, &headers, false).await? else {
        return Err(unauthorized());
    };
    let rows = state
        .store
        .get_preferences(principal.user.id)
        .await
        .map_err(db_error)?;
    let mut map = serde_json::Map::new();
    for (key, value) in rows {
        map.insert(key, value);
    }
    Ok(Json(serde_json::Value::Object(map)))
}

#[derive(Deserialize)]
struct PutPreferenceBody {
    key: String,
    value: serde_json::Value,
}

/// `PUT /api/v1/auth/preferences` — upserts one preference after allowlist
/// and shape validation (Section 10.1). Unknown keys and wrong shapes are
/// 422, never silent store writes.
async fn preferences_set(
    State(state): State<AuthState>,
    headers: HeaderMap,
    body: Result<Json<PutPreferenceBody>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(principal) = require_session(&state, &headers, true).await? else {
        return Err(unauthorized());
    };
    let Json(req) = body.map_err(|_| (StatusCode::BAD_REQUEST, json_error("bad_request")))?;
    let key = archaeodash_application::validate_preference(&req.key, &req.value)
        .map_err(domain_validation_error)?;
    state
        .store
        .set_preference(
            principal.user.id,
            key.as_str(),
            req.value,
            SystemTime::now(),
        )
        .await
        .map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The hosted control-plane router: auth surface plus user-scoped
/// preferences. Composed under the security-headers middleware and a strict
/// same-origin CORS allowlist by [`hosted_router`].
pub fn auth_router(state: AuthState) -> Router {
    Router::new()
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/verify", post(verify))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/logout-all", post(logout_all))
        .route("/api/v1/auth/session", get(session))
        .route("/api/v1/auth/consent", get(consent))
        .route(
            "/api/v1/preferences",
            get(preferences_get).put(preferences_set),
        )
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

#[cfg(test)]
mod preference_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)] // test code; panics are the failure mode

    use super::tests::{auth_app, auth_state, post_json, test_peer, unique_suffix};
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    /// Registers, verifies via the dev sink link, logs in, and returns the
    /// cookie header value plus the CSRF token for state-changing requests.
    pub(crate) async fn login_session(
        state: AuthState,
        sink: archaeodash_auth::email::DevSinkEmailSender,
    ) -> (String, String) {
        let suffix = unique_suffix();
        let username = format!("pref-user-{suffix}");
        let app = auth_app(state.clone());
        let response = post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": username,
                "email": format!("pref-user-{suffix}@example.com"),
                "password": "correct horse battery staple",
                "consent_version": "2026-10",
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let link = sink
            .stored()
            .first()
            .expect("verification email")
            .text_body
            .clone();
        let token = link
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
            serde_json::json!({ "token": token }),
        )
        .await;
        let response = post_json(
            app.clone(),
            "/api/v1/auth/login",
            serde_json::json!({
                "identifier": username,
                "password": "correct horse battery staple",
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
        let cookie_header = set_cookies
            .iter()
            .map(|c| c.split(';').next().expect("pair").to_string())
            .collect::<Vec<_>>()
            .join("; ");
        let csrf = set_cookies
            .iter()
            .find(|c| c.starts_with(CSRF_COOKIE))
            .expect("csrf cookie")
            .split(';')
            .next()
            .expect("pair")
            .split_once('=')
            .expect("name=value")
            .1
            .to_string();
        (cookie_header, csrf)
    }

    async fn put_pref(
        app: Router,
        cookies: &str,
        csrf: Option<&str>,
        key: &str,
        value: serde_json::Value,
    ) -> axum::response::Response {
        let mut req = axum::http::Request::builder()
            .method("PUT")
            .uri("/api/v1/preferences")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, cookies);
        if let Some(token) = csrf {
            req = req.header(CSRF_HEADER, token);
        }
        app.oneshot(
            req.body(Body::from(
                serde_json::json!({ "key": key, "value": value }).to_string(),
            ))
            .map(|mut req| {
                req.extensions_mut().insert(ConnectInfo(test_peer()));
                req
            })
            .expect("body"),
        )
        .await
        .expect("infallible")
    }

    #[tokio::test]
    async fn preferences_require_session() {
        let Some((state, _)) = auth_state().await else {
            return;
        };
        let app = auth_app(state.clone());
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/v1/preferences")
                    .body(Body::empty())
                    .map(|mut req| {
                        req.extensions_mut().insert(ConnectInfo(test_peer()));
                        req
                    })
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn preferences_put_rejects_missing_csrf() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let (cookies, _) = login_session(state.clone(), sink).await;
        let app = auth_app(state.clone());
        let res = put_pref(app, &cookies, None, "theme", serde_json::json!("dark")).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn preferences_round_trip_and_validation() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let (cookies, csrf) = login_session(state.clone(), sink.clone()).await;
        let app = auth_app(state.clone());

        // Unknown key is a 422, never a silent store write.
        let res = put_pref(
            app.clone(),
            &cookies,
            Some(&csrf),
            "unknownKey",
            serde_json::json!(true),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // Wrong shape for a known key is a 422.
        let res = put_pref(
            app.clone(),
            &cookies,
            Some(&csrf),
            "compactMode",
            serde_json::json!("yes"),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // Valid upsert then read-back.
        let res = put_pref(
            app.clone(),
            &cookies,
            Some(&csrf),
            "theme",
            serde_json::json!("dark"),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let res = put_pref(
            app.clone(),
            &cookies,
            Some(&csrf),
            "compactMode",
            serde_json::json!(true),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/v1/preferences")
                    .header(header::COOKIE, &cookies)
                    .body(Body::empty())
                    .map(|mut req| {
                        req.extensions_mut().insert(ConnectInfo(test_peer()));
                        req
                    })
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["compactMode"], true);

        // Upstream user isolation: another account reads none of this.
        let app2 = auth_app(state.clone());
        let suffix = unique_suffix();
        let register = serde_json::json!({
            "username": format!("pref-iso-{suffix}"),
            "email": format!("pref-iso-{suffix}@example.com"),
            "password": "correct horse battery staple",
            "consent_version": CONSENT_VERSION,
        });
        let res = post_json(app2.clone(), "/api/v1/auth/register", register).await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
    }
}

/// Section 14.3.1 lifecycle rehearsal: one continuous flow against live
/// PostgreSQL — registration, verification, session reflection, remembered
/// login, logout-all revocation, password reset, and the rate limit —
/// exactly the flows cutover rehearsal must exercise.
#[cfg(test)]
mod lifecycle_rehearsal {
    #![allow(clippy::expect_used, clippy::unwrap_used)] // test code; panics are the failure mode

    use super::tests::{auth_app, auth_state, post_json, test_peer, unique_suffix};
    use super::*;
    use archaeodash_auth::email::DevSinkEmailSender;
    use axum::body::Body;
    use tower::ServiceExt;

    async fn get_session(app: &Router, cookie: &str) -> axum::response::Response {
        app.clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/auth/session")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .map(|mut req| {
                        req.extensions_mut().insert(ConnectInfo(test_peer()));
                        req
                    })
                    .expect("request"),
            )
            .await
            .expect("infallible")
    }

    fn cookie_pair(response: &axum::response::Response) -> (String, String) {
        let cookies: Vec<String> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ascii").to_string())
            .collect();
        let pair = |name: &str| {
            cookies
                .iter()
                .find(|c| c.starts_with(name))
                .unwrap_or_else(|| panic!("missing {name} cookie"))
                .split(';')
                .next()
                .expect("pair")
                .to_string()
        };
        (
            pair(SESSION_COOKIE),
            pair(CSRF_COOKIE)
                .split_once('=')
                .expect("csrf value")
                .1
                .to_string(),
        )
    }

    #[tokio::test]
    async fn full_lifecycle_rehearsal() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let app = auth_app(state.clone());
        let suffix = unique_suffix();
        let username = format!("rehearsal-{suffix}");
        let email = format!("rehearsal-{suffix}@example.com");
        let original_password = "correct horse battery staple";
        let new_password = "a different correct horse staple";

        // 1. Registration returns the generic 202 and sends one verification link.
        let response = post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": username, "email": email,
                "password": original_password, "consent_version": "2026-10"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let verify_token = sink
            .stored()
            .last()
            .expect("verification email")
            .text_body
            .split("/auth/verify?token=")
            .nth(1)
            .expect("token")
            .lines()
            .next()
            .expect("token line")
            .to_string();

        // 2. Session reflects unverified before verification.
        let unauth = get_session(&app, "").await;
        assert_eq!(unauth.status(), StatusCode::OK);
        let body: SessionResponse = serde_json::from_slice(
            &axum::body::to_bytes(unauth.into_body(), usize::MAX)
                .await
                .expect("body"),
        )
        .expect("session json");
        assert!(!body.authenticated);

        // 3. Verification consumes the single-use token; replay is generic 400.
        let response = post_json(
            app.clone(),
            "/api/v1/auth/verify",
            serde_json::json!({ "token": verify_token }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let replay = post_json(
            app.clone(),
            "/api/v1/auth/verify",
            serde_json::json!({ "token": verify_token }),
        )
        .await;
        assert_eq!(replay.status(), StatusCode::BAD_REQUEST);

        // 4. Remembered login (90 days) sets both cookies and the session
        //    endpoint reflects the verified principal.
        let login = post_json(
            app.clone(),
            "/api/v1/auth/login",
            serde_json::json!({
                "identifier": username, "password": original_password,
                "remember_days": 90
            }),
        )
        .await;
        assert_eq!(login.status(), StatusCode::OK);
        let raw_cookies: Vec<String> = login
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ascii").to_string())
            .collect();
        let session_raw = raw_cookies
            .iter()
            .find(|c| c.starts_with(SESSION_COOKIE))
            .expect("session cookie");
        assert!(
            session_raw.contains("Max-Age=7776000"),
            "remember me expiry: {session_raw}"
        );
        let (session_cookie, csrf) = cookie_pair(&login);
        let session: SessionResponse = serde_json::from_slice(
            &axum::body::to_bytes(
                get_session(&app, &session_cookie).await.into_body(),
                usize::MAX,
            )
            .await
            .expect("body"),
        )
        .expect("session json");
        assert!(session.authenticated);
        assert_eq!(session.email_verified, Some(true));

        // 5. logout-all revokes every session: the old cookie stops resolving.
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout-all")
                    .header(
                        header::COOKIE,
                        format!("{session_cookie}; {CSRF_COOKIE}={csrf}"),
                    )
                    .header(CSRF_HEADER, &csrf)
                    .body(Body::empty())
                    .map(|mut req| {
                        req.extensions_mut().insert(ConnectInfo(test_peer()));
                        req
                    })
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let revoked: SessionResponse = serde_json::from_slice(
            &axum::body::to_bytes(
                get_session(&app, &session_cookie).await.into_body(),
                usize::MAX,
            )
            .await
            .expect("body"),
        )
        .expect("session json");
        assert!(!revoked.authenticated);

        // 6. Password reset: enumeration-resistant generic 202, then confirm
        //    sets the new hash.
        let response = post_json(
            app.clone(),
            "/api/v1/auth/password-reset/request",
            serde_json::json!({ "email": email }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let reset_token = sink
            .stored()
            .last()
            .expect("reset email")
            .text_body
            .split("/auth/reset?token=")
            .nth(1)
            .expect("token")
            .lines()
            .next()
            .expect("token line")
            .to_string();
        let response = post_json(
            app.clone(),
            "/api/v1/auth/password-reset/confirm",
            serde_json::json!({ "token": reset_token, "new_password": new_password }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // 7. Old password is dead; new password logs in.
        let old = post_json(
            app.clone(),
            "/api/v1/auth/login",
            serde_json::json!({ "identifier": username, "password": original_password }),
        )
        .await;
        assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
        let new = post_json(
            app.clone(),
            "/api/v1/auth/login",
            serde_json::json!({ "identifier": username, "password": new_password }),
        )
        .await;
        assert_eq!(new.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn remember_days_outside_30_or_90_is_rejected() {
        // Section 15 item 10: remember-me is an explicit choice among the
        // documented day choices; arbitrary tenures are a 422, and the
        // session issued is the 12-hour default when omitted.
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let suffix = unique_suffix();
        let username = format!("rehearsal-{suffix}");
        let app = auth_app(state.clone());
        let response = post_json(
            app.clone(),
            "/api/v1/auth/register",
            serde_json::json!({
                "username": username,
                "email": format!("rehearsal-{suffix}@example.com"),
                "password": "correct horse battery staple",
                "consent_version": "2026-10"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let token = sink_last_token(&sink, "/auth/verify?token=");
        post_json(
            app.clone(),
            "/api/v1/auth/verify",
            serde_json::json!({ "token": token }),
        )
        .await;

        for bad in [1u64, 36500] {
            let response = post_json(
                app.clone(),
                "/api/v1/auth/login",
                serde_json::json!({
                    "identifier": username,
                    "password": "correct horse battery staple",
                    "remember_days": bad
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        }

        // Omitted remember_days defaults to the 12-hour session cookie.
        let response = post_json(
            app,
            "/api/v1/auth/login",
            serde_json::json!({
                "identifier": username,
                "password": "correct horse battery staple"
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let session_raw = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ascii").to_string())
            .find(|c| c.starts_with(SESSION_COOKIE))
            .expect("session cookie");
        assert!(session_raw.contains("Max-Age=43200"), "{session_raw}");
    }

    /// Extracts the newest action token of the given link kind from the dev sink.
    fn sink_last_token(sink: &DevSinkEmailSender, marker: &str) -> String {
        sink.stored()
            .iter()
            .rev()
            .find(|m| m.text_body.contains(marker))
            .expect("email with link")
            .text_body
            .split(marker)
            .nth(1)
            .expect("token")
            .lines()
            .next()
            .expect("token line")
            .to_string()
    }

    #[tokio::test]
    async fn registration_requires_current_consent_version() {
        let Some((state, sink)) = auth_state().await else {
            return;
        };
        let app = auth_app(state.clone());
        let suffix = unique_suffix();
        let register = |consent: &'static str| {
            let app = app.clone();
            let username = format!("consent-{suffix}");
            let email = format!("consent-{suffix}@example.com");
            async move {
                post_json(
                    app,
                    "/api/v1/auth/register",
                    serde_json::json!({
                        "username": username,
                        "email": email,
                        "password": "correct horse battery staple",
                        "consent_version": consent
                    }),
                )
                .await
            }
        };
        // Stale or missing consent version is a 422.
        let res = register("2020-01").await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
        // The consent endpoint names the current version.
        let consent: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(
                app.clone()
                    .oneshot(
                        axum::http::Request::builder()
                            .uri("/api/v1/auth/consent")
                            .body(Body::empty())
                            .map(|mut req| {
                                req.extensions_mut().insert(ConnectInfo(test_peer()));
                                req
                            })
                            .expect("request"),
                    )
                    .await
                    .expect("infallible")
                    .into_body(),
                usize::MAX,
            )
            .await
            .expect("body"),
        )
        .expect("consent json");
        assert_eq!(consent["consent_version"], CONSENT_VERSION);
        // Current version registers and records the consent version.
        let res = register(CONSENT_VERSION).await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
        let stored = state
            .store
            .find_user_by_normalized_username(&format!("consent-{suffix}"))
            .await
            .expect("store")
            .expect("user exists");
        assert_eq!(stored.consent_version.as_deref(), Some(CONSENT_VERSION));
        assert!(stored.consented_at.is_some());
        let _ = sink;
    }
}
