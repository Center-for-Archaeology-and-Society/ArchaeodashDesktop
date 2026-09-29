//! Transport-neutral request and lifecycle DTOs for asynchronous Phase 6
//! analyses. Job state is project-scoped by the application adapter.

use serde::{Deserialize, Serialize};

use crate::{
    ClusterDiagnosticsRequest, ClusterDiagnosticsResponse, ClusterFitRequest, ClusterFitResponse,
    EuclideanMatchesRequest, EuclideanMatchesResponse, MembershipProbabilitiesRequest,
    MembershipProbabilitiesResponse,
};

/// Analysis operation submitted to the bounded local job registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case")]
pub enum AnalysisJobRequest {
    ClusterFit(ClusterFitRequest),
    ClusterDiagnostics(ClusterDiagnosticsRequest),
    MembershipProbabilities(MembershipProbabilitiesRequest),
    EuclideanMatches(EuclideanMatchesRequest),
}

/// Request envelope with an optional wall-clock deadline in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmitAnalysisJobRequest {
    pub analysis: AnalysisJobRequest,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Typed analysis output. It is populated only for succeeded jobs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "result", rename_all = "snake_case")]
pub enum AnalysisJobResult {
    ClusterFit(ClusterFitResponse),
    ClusterDiagnostics(ClusterDiagnosticsResponse),
    MembershipProbabilities(MembershipProbabilitiesResponse),
    EuclideanMatches(EuclideanMatchesResponse),
}

/// Stable externally visible job lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisJobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

/// Coarse progress stage suitable for both desktop and hosted clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisJobStage {
    Validating,
    ReadingRows,
    NormalizingSchema,
    Computing,
    Publishing,
}

/// Error data safe to show to a user. Internal diagnostics stay in logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisJobError {
    pub code: String,
    pub message: String,
}

/// A point-in-time job view. Timestamps are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisJobSnapshot {
    pub id: String,
    pub state: AnalysisJobState,
    pub stage: Option<AnalysisJobStage>,
    /// Percentage in the inclusive range 0..=100.
    pub progress: u8,
    pub submitted_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub completed_at_ms: Option<u64>,
    pub result: Option<AnalysisJobResult>,
    pub error: Option<AnalysisJobError>,
}
