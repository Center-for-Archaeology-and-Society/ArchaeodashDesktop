//! Bounded HTTP Server-Sent Events stream for lightweight job lifecycle updates.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use archaeodash_contracts::{
    AnalysisJobEvent, AnalysisJobSnapshot, AnalysisJobState, ErrorEnvelope,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use futures_util::stream;

use crate::AppState;

const MAX_SUBSCRIBERS: usize = 32;
const MAX_SUBSCRIBERS_PER_JOB: usize = 4;
const POLL_INTERVAL: Duration = Duration::from_millis(250);

static SUBSCRIBERS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

#[derive(Debug)]
struct Subscription(String);

impl Subscription {
    fn acquire(job_id: &str) -> Result<Self, (StatusCode, Json<ErrorEnvelope>)> {
        let subscribers = SUBSCRIBERS.get_or_init(|| Mutex::new(HashMap::new()));
        let Ok(mut counts) = subscribers.lock() else {
            return Err(error(
                StatusCode::SERVICE_UNAVAILABLE,
                "job_events_unavailable",
                "Job event service is unavailable",
            ));
        };
        let total: usize = counts.values().sum();
        let count = counts.get(job_id).copied().unwrap_or(0);
        if total >= MAX_SUBSCRIBERS || count >= MAX_SUBSCRIBERS_PER_JOB {
            return Err(error(
                StatusCode::TOO_MANY_REQUESTS,
                "job_event_limit",
                "Too many active job event streams",
            ));
        }
        *counts.entry(job_id.to_owned()).or_default() += 1;
        Ok(Self(job_id.to_owned()))
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(subscribers) = SUBSCRIBERS.get() {
            if let Ok(mut counts) = subscribers.lock() {
                if let Some(count) = counts.get_mut(&self.0) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        counts.remove(&self.0);
                    }
                }
            }
        }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<ErrorEnvelope>) {
    (
        status,
        Json(ErrorEnvelope {
            code: code.into(),
            message: message.into(),
        }),
    )
}

fn terminal(snapshot: &AnalysisJobSnapshot) -> bool {
    matches!(
        snapshot.state,
        AnalysisJobState::Succeeded
            | AnalysisJobState::Failed
            | AnalysisJobState::Cancelled
            | AnalysisJobState::TimedOut
    )
}

fn same_update(a: &AnalysisJobEvent, b: &AnalysisJobEvent) -> bool {
    a.id == b.id
        && a.state == b.state
        && a.stage == b.stage
        && a.progress == b.progress
        && a.error == b.error
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub async fn jobs_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(initial) = state.jobs.get(&id) else {
        return Err(error(
            StatusCode::NOT_FOUND,
            "job_not_found",
            "Analysis job not found or expired",
        ));
    };
    let subscription = Subscription::acquire(&id)?;
    let jobs = state.jobs;
    let stream = stream::unfold(
        (
            jobs,
            id,
            Some(initial),
            None::<AnalysisJobEvent>,
            Some(subscription),
        ),
        |(jobs, id, pending, last, subscription)| async move {
            let snapshot = if let Some(initial) = pending {
                initial
            } else {
                loop {
                    tokio::time::sleep(POLL_INTERVAL).await;
                    let snapshot = jobs.get(&id)?;
                    let event = AnalysisJobEvent::from(&snapshot);
                    if last
                        .as_ref()
                        .is_none_or(|previous| !same_update(previous, &event))
                    {
                        break snapshot;
                    }
                    if terminal(&snapshot) {
                        return None;
                    }
                }
            };
            let mut event = AnalysisJobEvent::from(&snapshot);
            event.updated_at_ms = now_ms();
            let encoded = match serde_json::to_string(&event) {
                Ok(payload) => Event::default()
                    .event("progress")
                    .id(event.updated_at_ms.to_string())
                    .data(payload),
                Err(_) => Event::default()
                    .event("error")
                    .data("{\"code\":\"job_event_encoding\"}"),
            };
            if terminal(&snapshot) {
                Some((
                    Ok::<Event, std::convert::Infallible>(encoded),
                    (jobs, id, None, Some(event), subscription),
                ))
            } else {
                Some((Ok(encoded), (jobs, id, None, Some(event), subscription)))
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use archaeodash_application::{
        AnalysisJobs, ClusterService, ExploreService, ExportService, GroupService, ImportService,
        OrdinationService, PreferenceService, SourceFileService, TransformService,
    };
    use archaeodash_contracts::{AnalysisJobRequest, ClusterFitRequest, SubmitAnalysisJobRequest};
    use http_body_util::BodyExt;
    use std::{sync::Arc, time::Instant};

    fn app_state(root: &std::path::Path) -> AppState {
        AppState {
            import: Arc::new(ImportService::new(root).unwrap()),
            groups: Arc::new(GroupService::new(root).unwrap()),
            files: Arc::new(SourceFileService::new(root).unwrap()),
            transforms: Arc::new(TransformService::new(root).unwrap()),
            ordination: Arc::new(OrdinationService::new(root).unwrap()),
            clustering: Arc::new(ClusterService::new(root).unwrap()),
            jobs: Arc::new(AnalysisJobs::new(root)),
            explore: Arc::new(ExploreService::new(root).unwrap()),
            exports: Arc::new(ExportService::new(root).unwrap()),
            preferences: Arc::new(PreferenceService::new(root).unwrap()),
        }
    }

    #[tokio::test]
    async fn terminal_sse_sends_lightweight_event_then_closes() {
        let dir = tempfile::tempdir().unwrap();
        let state = app_state(dir.path());
        let request: ClusterFitRequest = serde_json::from_value(serde_json::json!({
            "path":"missing.parquet", "columns":[], "method":"diana"
        }))
        .unwrap();
        let job = state
            .jobs
            .submit(SubmitAnalysisJobRequest {
                analysis: AnalysisJobRequest::ClusterFit(request),
                timeout_ms: Some(2_000),
            })
            .unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        let terminal_snapshot = loop {
            let current = state.jobs.get(&job.id).unwrap();
            if terminal(&current) {
                break current;
            }
            assert!(Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        assert_eq!(terminal_snapshot.state, AnalysisJobState::Failed);
        let response = jobs_events(State(state), Path(job.id))
            .await
            .unwrap()
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("event: progress"));
        assert!(text.contains("\"state\":\"failed\""));
        assert!(
            !text.contains("\"result\""),
            "event stream should omit the potentially large result"
        );
    }

    #[test]
    fn subscriptions_have_per_job_and_global_caps() {
        let id = format!("cap-test-{}", uuid::Uuid::now_v7());
        let mut guards = Vec::new();
        for _ in 0..MAX_SUBSCRIBERS_PER_JOB {
            guards.push(Subscription::acquire(&id).unwrap());
        }
        assert_eq!(
            Subscription::acquire(&id).unwrap_err().0,
            StatusCode::TOO_MANY_REQUESTS
        );
        drop(guards);
        assert!(
            Subscription::acquire(&id).is_ok(),
            "dropping a stream must release its slot"
        );
    }
}
