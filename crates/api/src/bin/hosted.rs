//! Hosted API server binary (Section 10 deployment shape): control-plane
//! Postgres, auth + preferences surface, health endpoints, correlation IDs,
//! and enforced security headers.
//!
//! Startup contract (Section 6.5): migrations run only when
//! `AUTH_APPLY_MIGRATIONS=1` (the explicit deployment step); otherwise the
//! binary performs a read-only schema compatibility check and exits if the
//! required migrations are absent. No DDL runs in user sessions.
//!
//! Email adapters (Section 11.1): `AUTH_EMAIL_MODE=sendmail` (default),
//! `smtp`, or `dev`. `dev` accepts links in-process and is refused unless
//! `AUTH_ALLOW_DEV_EMAIL=1` so it can never silently serve production.

use archaeodash_api::auth::AuthState;
use archaeodash_api::hosted_files::HostedFileStore;
use archaeodash_api::{finalize_hosted_router, hosted_router, HostedState};
use archaeodash_auth::email::{
    DevSinkEmailSender, EmailSender, SendmailEmailSender, SmtpEmailSender,
};
use archaeodash_auth::throttle::ThrottlePepper;
use archaeodash_control_postgres::ControlStore;
use std::sync::Arc;

fn env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("missing required environment variable {name}"))
}

fn build_email() -> Result<Arc<dyn EmailSender>, String> {
    let mode = std::env::var("AUTH_EMAIL_MODE").unwrap_or_else(|_| "sendmail".into());
    match mode.as_str() {
        "sendmail" => Ok(Arc::new(SendmailEmailSender::new())),
        "smtp" => Ok(Arc::new(SmtpEmailSender::new(
            env("AUTH_SMTP_HOST")?,
            env("AUTH_SMTP_PORT")?
                .parse()
                .map_err(|_| "AUTH_SMTP_PORT is not a number".to_string())?,
            env("AUTH_SMTP_USERNAME")?,
            env("AUTH_SMTP_PASSWORD")?,
            env("AUTH_SMTP_FROM")?,
        ))),
        "dev" => {
            if std::env::var("AUTH_ALLOW_DEV_EMAIL").as_deref() != Ok("1") {
                return Err(
                    "AUTH_EMAIL_MODE=dev requires AUTH_ALLOW_DEV_EMAIL=1 (never production)"
                        .to_string(),
                );
            }
            tracing::warn!("email dev sink active: verification/reset links are accepted in-process, not delivered");
            Ok(Arc::new(DevSinkEmailSender::new()))
        }
        other => Err(format!("unknown AUTH_EMAIL_MODE {other}")),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_format = std::env::var("AUTH_LOG_FORMAT").unwrap_or_else(|_| "json".into());
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    if log_format == "json" {
        subscriber.json().init();
    } else {
        subscriber.init();
    }

    let bind = std::env::var("AUTH_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let database_url = env("DATABASE_URL")?;
    let base_url = env("AUTH_BASE_URL")?;
    let pepper = ThrottlePepper::from_hex(&env("AUTH_PEPPER")?)?;
    let email = build_email()?;

    let store = ControlStore::connect(&database_url).await?;

    if std::env::var("AUTH_APPLY_MIGRATIONS").as_deref() == Ok("1") {
        store.migrate().await?;
        tracing::info!("control-plane migrations applied (explicit deployment step)");
    }
    if !store.schema_is_current().await? {
        return Err(
            "control-plane schema is not current: run migrations before serving (Section 6.5)"
                .to_string()
                .into(),
        );
    }

    let store = Arc::new(store);
    // Section 6.4: single-host deployments use the local filesystem backend
    // under AUTH_FILE_STORE_DIR; object keys are opaque UUID namespaces.
    let file_root =
        std::env::var("AUTH_FILE_STORE_DIR").unwrap_or_else(|_| "./data/user-files".into());
    let files = Arc::new(
        HostedFileStore::new(file_root).map_err(|e| format!("file store init failed: {e}"))?,
    );
    // Section 6.9: per-user logical byte quota for upload reservations.
    // Default 1 GiB — generous for INAA-scale spreadsheets, not unbounded.
    let quota_bytes: i64 = std::env::var("AUTH_QUOTA_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_073_741_824);
    let state = HostedState {
        auth: AuthState {
            store: store.clone(),
            email,
            pepper,
            base_url,
        },
        store: store.clone(),
        files: files.clone(),
        quota_bytes,
    };

    let app = finalize_hosted_router(hosted_router(state));

    // Retention sweep (Section 6.9): hourly, purges catalog tombstones older
    // than AUTH_RETENTION_DAYS (default 30) along with their trash objects.
    // Failures are logged and retried on the next tick; a crash between the
    // DB delete and the FS purge can only orphan an inaccessible trash file.
    let retention_days: i64 = std::env::var("AUTH_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let sweep_store = store.clone();
    let sweep_files = files.clone();
    // (clones taken before the state move below)
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let cutoff = time::OffsetDateTime::now_utc() - time::Duration::days(retention_days);
            match sweep_store.sweep_expired_tombstones(cutoff).await {
                Ok(swept) => {
                    for item in swept {
                        sweep_files.purge_trash_object(item.user_id, item.file_id);
                        tracing::info!(file_id = %item.file_id, "retention sweep purged file");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "retention sweep failed"),
            }
            // Transformation-definition tombstones sweep the same way; the
            // trash tag is the transformation ID, which the generic purge
            // matches.
            match sweep_store.sweep_expired_transformations(cutoff).await {
                Ok(swept) => {
                    for item in swept {
                        sweep_files.purge_trash_object(item.user_id, item.transformation_id);
                        tracing::info!(
                            transformation_id = %item.transformation_id,
                            "retention sweep purged transformation"
                        );
                    }
                }
                Err(e) => tracing::warn!(error = %e, "transformation retention sweep failed"),
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "hosted API listening");
    // Peer addresses feed the throttle keys; without connect info every
    // auth route would fail extraction (500) in production.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
