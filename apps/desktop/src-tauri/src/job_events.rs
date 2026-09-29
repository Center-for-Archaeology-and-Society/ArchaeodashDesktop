//! Bounded desktop event watcher for analysis job progress.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

use archaeodash_contracts::{AnalysisJobEvent, AnalysisJobState};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::DesktopState;

const MAX_WATCHERS: usize = 16;
const POLL_INTERVAL: Duration = Duration::from_millis(250);

static WATCHERS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn terminal(state: AnalysisJobState) -> bool {
    matches!(
        state,
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

fn registry() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    WATCHERS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[tauri::command]
pub(crate) fn watch_analysis_job(
    app: AppHandle,
    state: State<'_, Mutex<DesktopState>>,
    id: String,
) -> Result<(), String> {
    // Validate before reserving a watcher slot.
    state
        .lock()
        .map_err(|e| e.to_string())?
        .clustering
        .get_analysis_job(&id)?;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut watchers = registry().lock().map_err(|e| e.to_string())?;
        if let Some(existing) = watchers.get(&id) {
            if !existing.load(Ordering::Acquire) {
                return Ok(());
            }
            watchers.remove(&id);
        }
        if watchers.len() >= MAX_WATCHERS {
            return Err("Too many active job event watchers".into());
        }
        watchers.insert(id.clone(), Arc::clone(&stop));
    }
    let id_for_thread = id.clone();
    let spawned = thread::Builder::new()
        .name("analysis-job-events".into())
        .spawn(move || {
            let mut last: Option<AnalysisJobEvent> = None;
            while !stop.load(Ordering::Acquire) {
                let snapshot = app
                    .state::<Mutex<DesktopState>>()
                    .lock()
                    .ok()
                    .and_then(|state| state.clustering.get_analysis_job(&id_for_thread).ok());
                let Some(snapshot) = snapshot else {
                    break;
                };
                let is_terminal = terminal(snapshot.state);
                let mut event = AnalysisJobEvent::from(&snapshot);
                if last
                    .as_ref()
                    .is_some_and(|previous| same_update(previous, &event))
                {
                    if is_terminal {
                        break;
                    }
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }
                event.updated_at_ms = now_ms();
                last = Some(event.clone());
                if app.emit("analysis-job-progress", event).is_err() || is_terminal {
                    break;
                }
                thread::sleep(POLL_INTERVAL);
            }
            if let Ok(mut watchers) = registry().lock() {
                if watchers
                    .get(&id_for_thread)
                    .is_some_and(|current| Arc::ptr_eq(current, &stop))
                {
                    watchers.remove(&id_for_thread);
                }
            }
        });
    if let Err(err) = spawned {
        if let Ok(mut watchers) = registry().lock() {
            watchers.remove(&id);
        }
        return Err(err.to_string());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn stop_analysis_job_watch(id: String) -> Result<(), String> {
    if let Some(stop) = registry().lock().map_err(|e| e.to_string())?.get(&id) {
        stop.store(true, Ordering::Release);
    }
    Ok(())
}
