//! Application use cases shared by the HTTP and Tauri adapters (Section 1).
//!
//! Adapters call these use cases in-process; neither adapter owns business
//! logic and the desktop product never talks to a hidden loopback HTTP server.

use archaeodash_contracts::AppInfo;

pub mod explore;
pub mod files;
pub mod groups;
pub mod import;
pub mod ordination;
pub mod transforms;

pub use explore::ExploreService;
pub use files::SourceFileService;
pub use groups::GroupService;
pub use import::ImportService;
pub use ordination::OrdinationService;
pub use transforms::TransformService;

/// Returns application identity and readiness for the smoke use case.
///
/// This is the Phase 1 shared use case executed through both the Axum HTTP
/// adapter (`crates/api`) and the Tauri command adapter (`crates/desktop`).
pub fn app_info(transport: &str, ready: bool) -> AppInfo {
    AppInfo {
        app: "archaeodash".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        transport: transport.to_string(),
        ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_workspace_version() {
        let info = app_info("http", true);
        assert_eq!(info.app, "archaeodash");
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.transport, "http");
        assert!(info.ready);
    }
}
