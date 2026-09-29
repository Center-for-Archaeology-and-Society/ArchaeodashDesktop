//! Lightweight lifecycle updates emitted to job watchers. The completed
//! result remains available from the normal job snapshot endpoint.

use serde::{Deserialize, Serialize};

use crate::{AnalysisJobError, AnalysisJobSnapshot, AnalysisJobStage, AnalysisJobState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisJobEvent {
    pub id: String,
    pub state: AnalysisJobState,
    pub stage: Option<AnalysisJobStage>,
    pub progress: u8,
    pub updated_at_ms: u64,
    pub error: Option<AnalysisJobError>,
}

impl From<&AnalysisJobSnapshot> for AnalysisJobEvent {
    fn from(snapshot: &AnalysisJobSnapshot) -> Self {
        Self {
            id: snapshot.id.clone(),
            state: snapshot.state,
            stage: snapshot.stage,
            progress: snapshot.progress,
            updated_at_ms: snapshot
                .completed_at_ms
                .or(snapshot.started_at_ms)
                .unwrap_or(snapshot.submitted_at_ms),
            error: snapshot.error.clone(),
        }
    }
}
