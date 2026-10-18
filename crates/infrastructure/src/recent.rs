//! JSON-file store for the recent files list.

use application::AppError;
use application::recent::{RecentFile, RecentStore, UnixClock};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::file_store::replace_own_file;

/// Recent files kept in a JSON file inside the local app data folder.
#[derive(Debug, Clone)]
pub struct JsonRecentStore {
    path: PathBuf,
}

impl JsonRecentStore {
    /// Creates a store for the file at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl RecentStore for JsonRecentStore {
    fn load(&self) -> Result<Vec<RecentFile>, AppError> {
        match fs::read(&self.path) {
            // A damaged list is only a convenience; start over instead of failing the app.
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(AppError::from_io("read the recent files list", &e)),
        }
    }

    fn save(&self, files: &[RecentFile]) -> Result<(), AppError> {
        let bytes = serde_json::to_vec_pretty(files)
            .map_err(|_| AppError::internal("saving the recent files list"))?;
        replace_own_file(&self.path, &bytes)
    }
}

/// [`UnixClock`] over the system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemUnixClock;

impl UnixClock for SystemUnixClock {
    fn unix_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(name: &str) -> RecentFile {
        RecentFile {
            path: PathBuf::from(format!(r"C:\docs\{name}")),
            name: name.to_owned(),
            opened_at: 5,
        }
    }

    #[test]
    fn round_trips_and_starts_empty() {
        let dir = tempdir().unwrap();
        let store = JsonRecentStore::new(dir.path().join("recent.db"));
        assert!(store.load().unwrap().is_empty());
        store.save(&[entry("a.pdf"), entry("b.pdf")]).unwrap();
        assert_eq!(store.load().unwrap(), vec![entry("a.pdf"), entry("b.pdf")]);
    }

    #[test]
    fn a_damaged_file_loads_as_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("recent.db");
        fs::write(&path, b"{ not json").unwrap();
        assert!(JsonRecentStore::new(path).load().unwrap().is_empty());
    }

    #[test]
    fn the_clock_is_after_2026() {
        assert!(SystemUnixClock.unix_seconds() > 1_767_225_600);
    }
}
