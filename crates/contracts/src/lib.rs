//! Serde DTOs shared by the Axum HTTP API and the Tauri IPC adapter.
//!
//! Contracts are transport-neutral: both adapters serialize the same types so
//! the web and desktop clients cannot drift apart (Section 1).

use serde::{Deserialize, Serialize};

/// Smoke/health payload returned by `GET /healthz` and the Tauri `app_info`
/// command. Proves one use case flows through both adapters (Phase 1 exit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    /// Application name.
    pub app: String,
    /// Workspace version from Cargo.
    pub version: String,
    /// Adapter reporting the info, e.g. `http` or `tauri`.
    pub transport: String,
    /// Whether the hosted control plane is reachable (always `true` for desktop-local).
    pub ready: bool,
}

/// Transport-neutral error envelope using problem-details-style fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    /// Stable machine-readable error code.
    pub code: String,
    /// Safe user-facing message; diagnostics stay server-side.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_round_trips() {
        let info = AppInfo {
            app: "archaeodash".into(),
            version: "0.1.0".into(),
            transport: "http".into(),
            ready: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(serde_json::from_str::<AppInfo>(&json).unwrap(), info);
    }
}
