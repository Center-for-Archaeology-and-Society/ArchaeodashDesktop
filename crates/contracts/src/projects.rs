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
