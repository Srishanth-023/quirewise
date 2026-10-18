//! Structured JSON logging with daily rotation and 14-day retention.
//!
//! Never log document content or passwords. File paths are hashed at info level.

use application::AppError;
use application::ports::{LogLevel, Logger};
use sha2::{Digest, Sha256};
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};

/// Days of logs kept on disk.
pub const LOG_RETENTION_DAYS: usize = 14;

/// Short stable hash of a path for info-level logs (first 12 hex characters of SHA-256).
pub fn hash_path(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().to_lowercase().as_bytes());
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Keeps the background log writer alive. Drop it at exit to flush.
pub struct LogGuard {
    _guard: WorkerGuard,
}

/// Starts JSON logging into `logs_dir` and installs it as the global subscriber.
///
/// Calling it twice in one process is harmless: the second call keeps the first subscriber.
pub fn init_logging(logs_dir: &Path) -> Result<LogGuard, AppError> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("quirewise")
        .filename_suffix("log")
        .max_log_files(LOG_RETENTION_DAYS)
        .build(logs_dir)
        .map_err(|_| AppError::internal("opening the log folder"))?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .finish();
    // A global subscriber may already exist (tests, or a second init); that is fine.
    let _ = tracing::subscriber::set_global_default(subscriber);
    Ok(LogGuard { _guard: guard })
}

/// [`Logger`] port implementation that forwards to `tracing`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingLogger;

impl Logger for TracingLogger {
    fn log(&self, level: LogLevel, event: &str) {
        match level {
            LogLevel::Info => tracing::info!(event),
            LogLevel::Warn => tracing::warn!(event),
            LogLevel::Error => tracing::error!(event),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Buf {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn path_hashes_are_short_stable_and_case_insensitive() {
        let a = hash_path(Path::new(r"C:\Users\Me\Secret Plans.pdf"));
        let b = hash_path(Path::new(r"c:\users\me\secret plans.pdf"));
        assert_eq!(a, b);
        assert_eq!(a.len(), 12);
        assert!(!a.contains("Secret"));
    }

    #[test]
    fn tracing_logger_writes_json_events_without_extra_data() {
        let buf = Buf::default();
        let sink = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(move || sink.clone())
            .with_target(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            TracingLogger.log(LogLevel::Info, "job.finished");
        });
        let out = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        let line: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
        assert_eq!(line["fields"]["event"], "job.finished");
        assert_eq!(line["level"], "INFO");
    }

    #[test]
    fn init_logging_creates_the_log_folder_files() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        let guard = init_logging(&logs).unwrap();
        drop(guard);
        assert!(logs.is_dir());
    }
}
