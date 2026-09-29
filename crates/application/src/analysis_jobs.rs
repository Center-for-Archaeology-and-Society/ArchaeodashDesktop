//! Bounded project-local worker registry for long-running analyses.

use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use archaeodash_analysis::CancellationToken;
use archaeodash_contracts::{
    AnalysisJobError, AnalysisJobRequest, AnalysisJobResult, AnalysisJobSnapshot, AnalysisJobStage,
    AnalysisJobState, SubmitAnalysisJobRequest,
};
use archaeodash_domain::DomainError;
use uuid::Uuid;

const DEFAULT_WORKERS: usize = 2;
const DEFAULT_QUEUE_CAPACITY: usize = 16;
const MAX_RETAINED_JOBS: usize = 32;
const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_TIMEOUT: Duration = Duration::from_secs(1800);

type JobTask = Box<dyn FnOnce(JobContext) -> Result<AnalysisJobResult, DomainError> + Send>;

struct Record {
    snapshot: AnalysisJobSnapshot,
    token: CancellationToken,
}

struct WorkItem {
    id: String,
    token: CancellationToken,
    deadline: Option<Instant>,
    record: Arc<Mutex<Record>>,
    task: JobTask,
}

/// Typed Phase 6 job service bound to one project directory.
#[derive(Clone)]
pub struct AnalysisJobs {
    root: std::path::PathBuf,
    registry: AnalysisJobRegistry,
}

impl AnalysisJobs {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            root: root.into(),
            registry: AnalysisJobRegistry::new(),
        }
    }

    pub fn submit(
        &self,
        request: SubmitAnalysisJobRequest,
    ) -> Result<AnalysisJobSnapshot, DomainError> {
        let root = self.root.clone();
        self.registry
            .submit(request.timeout_ms.map(Duration::from_millis), move |ctx| {
                ctx.progress(AnalysisJobStage::ReadingRows, 5);
                ctx.checkpoint()?;
                let service = crate::ClusterService::new(root)?;
                ctx.progress(AnalysisJobStage::Computing, 15);
                let token = ctx.cancellation_token();
                let result = match request.analysis {
                    AnalysisJobRequest::ClusterFit(req) => AnalysisJobResult::ClusterFit(
                        service.cluster_fit_cancellable(&req, &token)?,
                    ),
                    AnalysisJobRequest::ClusterDiagnostics(req) => {
                        AnalysisJobResult::ClusterDiagnostics(
                            service.cluster_diagnostics_cancellable(&req, &token)?,
                        )
                    }
                    AnalysisJobRequest::MembershipProbabilities(req) => {
                        AnalysisJobResult::MembershipProbabilities(
                            service.membership_probabilities_cancellable(&req, &token)?,
                        )
                    }
                    AnalysisJobRequest::EuclideanMatches(req) => {
                        AnalysisJobResult::EuclideanMatches(
                            service.euclidean_matches_cancellable(&req, &token)?,
                        )
                    }
                };
                ctx.checkpoint()?;
                Ok(result)
            })
    }

    pub fn get(&self, id: &str) -> Option<AnalysisJobSnapshot> {
        self.registry.get(id)
    }
    pub fn cancel(&self, id: &str) -> Result<Option<AnalysisJobSnapshot>, DomainError> {
        self.registry.cancel(id)
    }
    pub fn cancel_all(&self) {
        self.registry.cancel_all();
    }
}

/// Shared project-local registry. The worker count and queued work are both
/// bounded; dropping the last registry handle shuts down its workers.
#[derive(Clone)]
pub struct AnalysisJobRegistry {
    owner: Arc<RegistryOwner>,
    sender: SyncSender<WorkItem>,
    workers: usize,
}

struct RegistryOwner {
    records: Arc<Mutex<HashMap<String, Arc<Mutex<Record>>>>>,
}

impl Drop for RegistryOwner {
    fn drop(&mut self) {
        if let Ok(records) = self.records.lock() {
            for record in records.values() {
                if let Ok(mut r) = record.lock() {
                    if matches!(
                        r.snapshot.state,
                        AnalysisJobState::Queued | AnalysisJobState::Running
                    ) {
                        r.token.cancel();
                        if r.snapshot.state == AnalysisJobState::Queued {
                            terminal(&mut r, AnalysisJobState::Cancelled, None, None);
                        }
                    }
                }
            }
        }
    }
}

/// Cancellation and progress handle available to an analysis worker.
#[derive(Clone)]
pub struct JobContext {
    id: String,
    token: CancellationToken,
    deadline: Option<Instant>,
    record: Arc<Mutex<Record>>,
}

impl JobContext {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns true after explicit cancellation or the job deadline.
    pub fn is_cancelled(&self) -> bool {
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            self.token.cancel();
        }
        self.token.is_cancelled()
    }

    /// A cheap cooperative checkpoint for loops and stage boundaries.
    pub fn checkpoint(&self) -> Result<(), DomainError> {
        if self.is_cancelled() {
            Err(DomainError::validation(
                "analysis_cancelled",
                "analysis was cancelled or exceeded its deadline",
            ))
        } else {
            Ok(())
        }
    }

    /// Publishes a bounded progress update.
    pub fn progress(&self, stage: AnalysisJobStage, percent: u8) {
        if let Ok(mut record) = self.record.lock() {
            if record.snapshot.state == AnalysisJobState::Running {
                record.snapshot.stage = Some(stage);
                record.snapshot.progress = percent.min(99);
            }
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.token.clone()
    }
}

impl AnalysisJobRegistry {
    /// Creates a bounded pool (two CPU workers and sixteen queued jobs).
    pub fn new() -> Self {
        let workers = thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .clamp(1, DEFAULT_WORKERS);
        Self::with_limits(workers, DEFAULT_QUEUE_CAPACITY)
    }

    /// Creates a pool with explicit limits, useful for machine-aware desktop
    /// sizing and deterministic concurrency tests. Both limits must be > 0.
    pub fn with_limits(workers: usize, queue_capacity: usize) -> Self {
        assert!(
            workers > 0 && queue_capacity > 0,
            "job limits must be positive"
        );
        let (sender, receiver) = mpsc::sync_channel::<WorkItem>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let records = Arc::new(Mutex::new(HashMap::new()));
        let mut started_workers = 0;
        for _ in 0..workers {
            let receiver = Arc::clone(&receiver);
            match thread::Builder::new()
                .name("analysis-job-worker".into())
                .spawn(move || loop {
                    let item = match receiver.lock().ok().and_then(|r| r.recv().ok()) {
                        Some(item) => item,
                        None => break,
                    };
                    run_item(item);
                }) {
                Ok(_) => started_workers += 1,
                Err(_) => break,
            }
        }
        Self {
            owner: Arc::new(RegistryOwner { records }),
            sender,
            workers: started_workers,
        }
    }

    /// Queues one job. The callback must use `checkpoint` or the supplied
    /// token in long loops so cancellation stops the underlying computation.
    pub fn submit<F>(
        &self,
        timeout: Option<Duration>,
        task: F,
    ) -> Result<AnalysisJobSnapshot, DomainError>
    where
        F: FnOnce(JobContext) -> Result<AnalysisJobResult, DomainError> + Send + 'static,
    {
        if self.workers == 0 {
            return Err(internal("analysis workers are unavailable"));
        }
        let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);
        if timeout.is_zero() || timeout > MAX_TIMEOUT {
            return Err(DomainError::validation(
                "analysis_timeout_range",
                "timeout_ms must lie between 1 and 1800000",
            ));
        }
        let now = now_ms();
        let id = Uuid::now_v7().to_string();
        let token = CancellationToken::new();
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            DomainError::validation("analysis_timeout_range", "timeout is out of range")
        })?;
        token.set_deadline(deadline);
        let record = Arc::new(Mutex::new(Record {
            snapshot: AnalysisJobSnapshot {
                id: id.clone(),
                state: AnalysisJobState::Queued,
                stage: Some(AnalysisJobStage::Validating),
                progress: 0,
                submitted_at_ms: now,
                started_at_ms: None,
                completed_at_ms: None,
                result: None,
                error: None,
            },
            token: token.clone(),
        }));
        {
            let mut records = self
                .owner
                .records
                .lock()
                .map_err(|_| internal("job registry poisoned"))?;
            if records.len() >= MAX_RETAINED_JOBS {
                let evict = records
                    .iter()
                    .filter_map(|(job_id, old)| {
                        let r = old.lock().ok()?;
                        matches!(
                            r.snapshot.state,
                            AnalysisJobState::Succeeded
                                | AnalysisJobState::Failed
                                | AnalysisJobState::Cancelled
                                | AnalysisJobState::TimedOut
                        )
                        .then_some((
                            job_id.clone(),
                            r.snapshot
                                .completed_at_ms
                                .unwrap_or(r.snapshot.submitted_at_ms),
                        ))
                    })
                    .min_by_key(|(_, at)| *at)
                    .map(|(job_id, _)| job_id);
                if let Some(oldest) = evict {
                    records.remove(&oldest);
                } else {
                    return Err(DomainError::validation(
                        "analysis_registry_full",
                        "too many active analysis jobs",
                    ));
                }
            }
            records.insert(id.clone(), Arc::clone(&record));
        }
        let item = WorkItem {
            id: id.clone(),
            token,
            deadline: Some(deadline),
            record: Arc::clone(&record),
            task: Box::new(task),
        };
        match self.sender.try_send(item) {
            Ok(()) => snapshot_of(&record),
            Err(TrySendError::Full(_)) => {
                self.owner
                    .records
                    .lock()
                    .ok()
                    .and_then(|mut m| m.remove(&id));
                Err(DomainError::validation(
                    "analysis_queue_full",
                    "analysis queue is full; retry later",
                ))
            }
            Err(TrySendError::Disconnected(_)) => {
                self.owner
                    .records
                    .lock()
                    .ok()
                    .and_then(|mut m| m.remove(&id));
                Err(internal("analysis workers are unavailable"))
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<AnalysisJobSnapshot> {
        let record = self.owner.records.lock().ok()?.get(id)?.clone();
        snapshot_of(&record).ok()
    }

    /// Requests actual cooperative cancellation. Queued work is terminalized
    /// immediately and skipped when a worker receives it.
    pub fn cancel(&self, id: &str) -> Result<Option<AnalysisJobSnapshot>, DomainError> {
        let record = self
            .owner
            .records
            .lock()
            .map_err(|_| internal("job registry poisoned"))?
            .get(id)
            .cloned();
        let Some(record) = record else {
            return Ok(None);
        };
        let mut r = record.lock().map_err(|_| internal("job record poisoned"))?;
        match r.snapshot.state {
            AnalysisJobState::Queued | AnalysisJobState::Running => {
                r.token.cancel();
                if r.snapshot.state == AnalysisJobState::Queued {
                    terminal(&mut r, AnalysisJobState::Cancelled, None, None);
                }
            }
            _ => {}
        }
        Ok(Some(r.snapshot.clone()))
    }

    pub fn cancel_all(&self) {
        if let Ok(records) = self.owner.records.lock() {
            for record in records.values() {
                if let Ok(mut r) = record.lock() {
                    if matches!(
                        r.snapshot.state,
                        AnalysisJobState::Queued | AnalysisJobState::Running
                    ) {
                        r.token.cancel();
                        if r.snapshot.state == AnalysisJobState::Queued {
                            terminal(&mut r, AnalysisJobState::Cancelled, None, None);
                        }
                    }
                }
            }
        }
    }
}

impl Default for AnalysisJobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn run_item(item: WorkItem) {
    let record = Arc::clone(&item.record);
    {
        let Ok(mut r) = record.lock() else {
            return;
        };
        if r.snapshot.state != AnalysisJobState::Queued {
            return;
        }
        if item.deadline.is_some_and(|d| Instant::now() >= d) {
            terminal(
                &mut r,
                AnalysisJobState::TimedOut,
                None,
                Some(error(
                    "analysis_timed_out",
                    "analysis exceeded its deadline",
                )),
            );
            return;
        }
        if item.token.is_cancelled() {
            terminal(&mut r, AnalysisJobState::Cancelled, None, None);
            return;
        }
        r.snapshot.state = AnalysisJobState::Running;
        r.snapshot.stage = Some(AnalysisJobStage::Validating);
        r.snapshot.started_at_ms = Some(now_ms());
    }
    let ctx = JobContext {
        id: item.id,
        token: item.token.clone(),
        deadline: item.deadline,
        record: Arc::clone(&record),
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| (item.task)(ctx)));
    let Ok(mut r) = record.lock() else {
        return;
    };
    if item.deadline.is_some_and(|d| Instant::now() >= d) {
        item.token.cancel();
        terminal(
            &mut r,
            AnalysisJobState::TimedOut,
            None,
            Some(error(
                "analysis_timed_out",
                "analysis exceeded its deadline",
            )),
        );
    } else if item.token.is_cancelled() {
        terminal(&mut r, AnalysisJobState::Cancelled, None, None);
    } else {
        match outcome {
            Ok(Ok(result)) => match serde_json::to_vec(&result) {
                Ok(bytes) if bytes.len() <= MAX_RESULT_BYTES => {
                    terminal(&mut r, AnalysisJobState::Succeeded, Some(result), None)
                }
                Ok(_) => terminal(
                    &mut r,
                    AnalysisJobState::Failed,
                    None,
                    Some(error(
                        "analysis_output_limit",
                        "analysis output exceeds the 8 MiB job result limit",
                    )),
                ),
                Err(_) => terminal(
                    &mut r,
                    AnalysisJobState::Failed,
                    None,
                    Some(error(
                        "analysis_output_invalid",
                        "analysis output could not be encoded",
                    )),
                ),
            },
            Ok(Err(err)) => terminal(
                &mut r,
                AnalysisJobState::Failed,
                None,
                Some(domain_error(err)),
            ),
            Err(_) => terminal(
                &mut r,
                AnalysisJobState::Failed,
                None,
                Some(error(
                    "analysis_job_panicked",
                    "analysis failed unexpectedly",
                )),
            ),
        }
    }
}

fn snapshot_of(record: &Arc<Mutex<Record>>) -> Result<AnalysisJobSnapshot, DomainError> {
    record
        .lock()
        .map(|r| r.snapshot.clone())
        .map_err(|_| internal("job record poisoned"))
}

fn terminal(
    r: &mut Record,
    state: AnalysisJobState,
    result: Option<AnalysisJobResult>,
    error: Option<AnalysisJobError>,
) {
    r.snapshot.state = state;
    r.snapshot.stage = None;
    r.snapshot.progress = if state == AnalysisJobState::Succeeded {
        100
    } else {
        r.snapshot.progress
    };
    r.snapshot.completed_at_ms = Some(now_ms());
    r.snapshot.result = result;
    r.snapshot.error = error;
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn internal(message: &str) -> DomainError {
    DomainError::validation("analysis_job_internal", message)
}
fn error(code: &str, message: &str) -> AnalysisJobError {
    AnalysisJobError {
        code: code.into(),
        message: message.into(),
    }
}
fn domain_error(err: DomainError) -> AnalysisJobError {
    match err {
        DomainError::Validation { code, message } => AnalysisJobError { code, message },
        DomainError::InvalidIdentity { message } => error("invalid_identity", &message),
        DomainError::NotFound(_) => error("analysis_not_found", "analysis input was not found"),
        DomainError::Internal(_) => error("analysis_failed", "analysis could not be completed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archaeodash_contracts::{ClusterFitResponse, ClusterMethod};
    use std::{sync::mpsc, thread::sleep};

    fn success() -> AnalysisJobResult {
        AnalysisJobResult::ClusterFit(ClusterFitResponse {
            path: "groups/a.parquet".into(),
            analytical_uuids: vec![],
            revision_id: "r1".into(),
            method: ClusterMethod::Diana,
            source: Default::default(),
            column_names: vec![],
            plot_column_names: vec![],
            plot_coordinates: vec![],
            plot_groups: vec![],
            cluster_plot_coordinates: vec![],
            cluster_plot_column_names: vec![],
            plot_warning: None,
            metric: Default::default(),
            linkage: Default::default(),
            n_rows: 0,
            cluster: None,
            size: None,
            tot_withinss: None,
            centers: None,
            medoids: None,
            merge: None,
            height: None,
            order: None,
            silhouette: None,
        })
    }

    fn wait_terminal(registry: &AnalysisJobRegistry, id: &str) -> AnalysisJobSnapshot {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = registry.get(id).expect("job retained");
            if matches!(
                snapshot.state,
                AnalysisJobState::Succeeded
                    | AnalysisJobState::Failed
                    | AnalysisJobState::Cancelled
                    | AnalysisJobState::TimedOut
            ) {
                return snapshot;
            }
            assert!(Instant::now() < until, "job did not finish: {snapshot:?}");
            sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn job_lifecycle_reports_success_and_failure() {
        let jobs = AnalysisJobRegistry::with_limits(1, 2);
        let queued = jobs.submit(None, |_| Ok(success())).unwrap();
        assert!(matches!(
            queued.state,
            AnalysisJobState::Queued | AnalysisJobState::Running
        ));
        let done = wait_terminal(&jobs, &queued.id);
        assert_eq!(done.state, AnalysisJobState::Succeeded);
        assert_eq!(done.progress, 100);
        assert!(done.result.is_some());

        let failed = jobs
            .submit(None, |_| {
                Err(DomainError::validation("bad_input", "invalid"))
            })
            .unwrap();
        let failed = wait_terminal(&jobs, &failed.id);
        assert_eq!(failed.state, AnalysisJobState::Failed);
        assert_eq!(failed.error.unwrap().code, "bad_input");
    }

    #[test]
    fn cancellation_stops_cooperative_work_and_deadline_times_out() {
        let jobs = AnalysisJobRegistry::with_limits(1, 2);
        let running = jobs
            .submit(None, |ctx| loop {
                if ctx.checkpoint().is_err() {
                    return Err(DomainError::validation("analysis_cancelled", "cancelled"));
                }
                thread::yield_now();
            })
            .unwrap();
        let until = Instant::now() + Duration::from_secs(1);
        while jobs.get(&running.id).unwrap().state != AnalysisJobState::Running {
            assert!(Instant::now() < until);
            sleep(Duration::from_millis(1));
        }
        jobs.cancel(&running.id).unwrap();
        assert_eq!(
            wait_terminal(&jobs, &running.id).state,
            AnalysisJobState::Cancelled
        );

        let timed = jobs
            .submit(Some(Duration::from_millis(15)), |ctx| loop {
                if ctx.checkpoint().is_err() {
                    return Ok(success());
                }
                thread::yield_now();
            })
            .unwrap();
        assert_eq!(
            wait_terminal(&jobs, &timed.id).state,
            AnalysisJobState::TimedOut
        );
    }

    #[test]
    fn queue_and_registry_are_bounded() {
        let jobs = AnalysisJobRegistry::with_limits(1, 1);
        let (release_tx, release_rx) = mpsc::channel();
        let first = jobs
            .submit(None, move |_| {
                let _ = release_rx.recv();
                Ok(success())
            })
            .unwrap();
        let until = Instant::now() + Duration::from_secs(1);
        while jobs.get(&first.id).unwrap().state != AnalysisJobState::Running {
            assert!(Instant::now() < until);
            sleep(Duration::from_millis(1));
        }
        let _queued = jobs.submit(None, |_| Ok(success())).unwrap();
        assert_eq!(
            jobs.submit(None, |_| Ok(success()))
                .unwrap_err()
                .to_string()
                .contains("queue is full"),
            true
        );
        release_tx.send(()).unwrap();
        assert_eq!(
            wait_terminal(&jobs, &first.id).state,
            AnalysisJobState::Succeeded
        );
    }

    #[test]
    fn dropping_project_registry_cancels_running_work() {
        let jobs = AnalysisJobRegistry::with_limits(1, 1);
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let submitted = jobs
            .submit(None, move |ctx| loop {
                if ctx.checkpoint().is_err() {
                    let _ = stopped_tx.send(());
                    return Err(DomainError::validation("analysis_cancelled", "cancelled"));
                }
                thread::yield_now();
            })
            .unwrap();
        let until = Instant::now() + Duration::from_secs(1);
        while jobs.get(&submitted.id).unwrap().state != AnalysisJobState::Running {
            assert!(Instant::now() < until);
            sleep(Duration::from_millis(1));
        }
        drop(jobs);
        stopped_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }
}
