//! The desktop's recent-projects list: the last [`MAX_RECENT`] project
//! directories the user opened, most recent first, stored as JSON in the
//! application's config directory (never inside a project).
//!
//! Security (IMPLEMENTATION.md Section 11 desktop rule: JavaScript never
//! supplies a filesystem path): a project may be reopened only if its path is
//! already on this list, which only the native folder picker can add to.

use archaeodash_contracts::RecentProject;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Entries kept; older ones fall off the end.
pub const MAX_RECENT: usize = 10;

const FILE_NAME: &str = "recent_projects.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredEntry {
    path: String,
    name: String,
    last_opened_unix_secs: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredList {
    projects: Vec<StoredEntry>,
}

/// File-backed recent-projects list. Every operation reads and rewrites the
/// file, so several windows or a crash never lose more than one update.
#[derive(Debug, Clone)]
pub struct RecentProjects {
    file: PathBuf,
}

impl RecentProjects {
    /// A list stored in `config_dir` (created on first write).
    pub fn new(config_dir: impl AsRef<Path>) -> Self {
        Self {
            file: config_dir.as_ref().join(FILE_NAME),
        }
    }

    fn read(&self) -> StoredList {
        // A missing or damaged file is an empty list, never an error.
        std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(&self, list: &StoredList) -> Result<(), String> {
        let dir = self
            .file
            .parent()
            .ok_or_else(|| "recent projects file has no parent directory".to_string())?;
        std::fs::create_dir_all(dir).map_err(|e| format!("could not save recent projects: {e}"))?;
        let bytes = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!(".{FILE_NAME}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| format!("could not save recent projects: {e}"))?;
        std::fs::rename(&tmp, &self.file)
            .map_err(|e| format!("could not save recent projects: {e}"))
    }

    /// The list, most recent first, with each directory's availability.
    pub fn list(&self) -> Vec<RecentProject> {
        self.read()
            .projects
            .into_iter()
            .map(|e| RecentProject {
                available: Path::new(&e.path).is_dir(),
                path: e.path,
                name: e.name,
                last_opened_unix_secs: e.last_opened_unix_secs,
            })
            .collect()
    }

    /// Whether `path` is on the list (the gate for reopening by path).
    pub fn contains(&self, path: &str) -> bool {
        self.read().projects.iter().any(|e| e.path == path)
    }

    /// Moves (or adds) `path` to the top, stamped `now_unix_secs`.
    pub fn record(&self, path: &str, name: &str, now_unix_secs: u64) -> Result<(), String> {
        let mut list = self.read();
        list.projects.retain(|e| e.path != path);
        list.projects.insert(
            0,
            StoredEntry {
                path: path.to_string(),
                name: name.to_string(),
                last_opened_unix_secs: now_unix_secs,
            },
        );
        list.projects.truncate(MAX_RECENT);
        self.write(&list)
    }

    /// Removes one entry (the directory itself is untouched).
    pub fn remove(&self, path: &str) -> Result<Vec<RecentProject>, String> {
        let mut list = self.read();
        list.projects.retain(|e| e.path != path);
        self.write(&list)?;
        Ok(self.list())
    }

    /// Empties the list.
    pub fn clear(&self) -> Result<(), String> {
        self.write(&StoredList::default())
    }
}

/// Unix seconds now (0 before the epoch).
pub fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("archaeodash-recent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn records_most_recent_first_without_duplicates_and_caps_the_list() {
        let dir = temp_dir("order");
        let projects: Vec<PathBuf> = (0..12)
            .map(|i| {
                let p = dir.join(format!("p{i}"));
                std::fs::create_dir_all(&p).expect("project dir");
                p
            })
            .collect();
        let recent = RecentProjects::new(dir.join("config"));
        assert!(recent.list().is_empty(), "no file yet means an empty list");
        for (i, p) in projects.iter().enumerate() {
            recent
                .record(&p.to_string_lossy(), &format!("p{i}"), 100 + i as u64)
                .expect("record");
        }
        // Reopening an older project moves it to the top instead of duplicating it.
        recent
            .record(&projects[5].to_string_lossy(), "p5", 500)
            .expect("reopen");
        let list = recent.list();
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].name, "p5");
        assert_eq!(list[0].last_opened_unix_secs, 500);
        assert_eq!(list[1].name, "p11");
        assert_eq!(list.iter().filter(|e| e.name == "p5").count(), 1);
        assert!(list.iter().all(|e| e.available));
        assert!(
            !list.iter().any(|e| e.name == "p0" || e.name == "p1"),
            "oldest fall off"
        );
    }

    #[test]
    fn only_listed_paths_can_be_reopened_and_missing_folders_are_flagged() {
        let dir = temp_dir("gate");
        let project = dir.join("site-a");
        std::fs::create_dir_all(&project).expect("project dir");
        let recent = RecentProjects::new(dir.join("config"));
        let path = project.to_string_lossy().into_owned();
        recent.record(&path, "site-a", 1).expect("record");
        assert!(recent.contains(&path));
        assert!(!recent.contains("/etc"), "unlisted paths are refused");
        std::fs::remove_dir_all(&project).expect("remove project");
        assert!(
            !recent.list()[0].available,
            "a deleted folder is flagged, not dropped"
        );
        assert!(recent.remove(&path).expect("remove").is_empty());
        recent.record(&path, "site-a", 2).expect("record again");
        recent.clear().expect("clear");
        assert!(recent.list().is_empty());
    }

    #[test]
    fn a_damaged_file_reads_as_an_empty_list() {
        let dir = temp_dir("damaged");
        std::fs::write(dir.join(FILE_NAME), b"{not json").expect("write");
        let recent = RecentProjects::new(&dir);
        assert!(recent.list().is_empty());
        recent
            .record("/tmp/x", "x", 1)
            .expect("record repairs the file");
        assert_eq!(recent.list().len(), 1);
    }
}
