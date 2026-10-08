//! Desktop logging (Section 12: rotating desktop logs with level config).
//! Human-readable output plus a daily-rotating file in the platform log
//! directory, retained for a bounded number of days. Level configuration
//! comes from `RUST_LOG` (default `info`). The subscriber is process-global;
//! `run` initializes it once before commands execute.

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, EnvFilter};

/// Retention for rotated desktop log files.
const MAX_LOG_FILES: usize = 14;

/// Level configuration (Section 12): `RUST_LOG` when set, `info` otherwise.
fn build_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Resolves the log destination. A creatable directory selects daily
/// rotation with bounded retention and returns the worker guard (flush on
/// drop, must outlive the process); anything else degrades to stdout-only
/// without panicking at startup.
fn resolve_sink(log_dir: Option<&std::path::Path>) -> Option<WorkerGuard> {
    let dir = log_dir?;
    std::fs::create_dir_all(dir).ok()?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("archaeodash")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    fmt()
        .with_env_filter(build_filter())
        .with_ansi(false)
        .with_target(false)
        .with_writer(writer)
        .try_init()
        .ok()?;
    Some(guard)
}

/// Installs the global desktop subscriber. Returns the rotating-file worker
/// guard when file logging is active; `run` must keep it alive.
pub fn init(log_dir: Option<&std::path::Path>) -> Option<WorkerGuard> {
    match resolve_sink(log_dir) {
        Some(guard) => Some(guard),
        // Stdout-only fallback (no directory, or the file subscriber was
        // already installed by an earlier call in this process).
        None => {
            fmt()
                .with_env_filter(build_filter())
                .with_ansi(false)
                .with_target(false)
                .try_init()
                .ok();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;

    #[test]
    fn default_level_is_info_and_env_overrides() {
        // RUST_LOG unset -> info; set -> honored (Section 12 level config).
        std::env::remove_var("RUST_LOG");
        let filter = build_filter().max_level_hint();
        assert_eq!(filter, Some(tracing::level_filters::LevelFilter::INFO));
        std::env::set_var("RUST_LOG", "debug");
        let filter = build_filter().max_level_hint();
        assert_eq!(filter, Some(tracing::level_filters::LevelFilter::DEBUG));
        std::env::remove_var("RUST_LOG");
    }

    #[test]
    fn missing_log_dir_degrades_to_stdout_without_panicking() {
        // The guard is None when no file appender could be set up; the
        // process still logs (the caller's stdout fallback). A second call
        // after a global subscriber exists is also a safe no-op.
        //
        // The path must be uncreatable for ANY user, including root in a
        // container: a regular file is created where the log directory is
        // expected, so `create_dir_all` fails with ENOTDIR regardless of
        // privileges (a path like /nonexistent/xyz would be creatable by
        // root and make this test environment-dependent).
        let not_a_dir = std::env::temp_dir().join("archaeodash-log-test-not-a-dir");
        let _ = std::fs::remove_file(&not_a_dir);
        std::fs::write(&not_a_dir, b"file, not directory").expect("write blocking file");
        assert!(init(Some(&not_a_dir.join("logs"))).is_none());
        std::fs::remove_file(&not_a_dir).ok();
        assert!(init(None).is_none());
    }
}
