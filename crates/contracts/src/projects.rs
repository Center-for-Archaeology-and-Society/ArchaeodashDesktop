//! Desktop-local project selection state.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    /// Canonical local project directory path.
    pub path: String,
    /// Directory name shown in the desktop shell.
    pub name: String,
    /// Changes on every successful project open, including reopening the same directory.
    pub generation: u64,
}

/// One entry in the desktop's recent-projects list (most recent first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProject {
    /// Canonical project directory path, exactly as opened.
    pub path: String,
    /// Directory name shown in the shell.
    pub name: String,
    /// Unix seconds of the last successful open.
    pub last_opened_unix_secs: u64,
    /// Whether the directory still exists (checked when listed).
    pub available: bool,
}
