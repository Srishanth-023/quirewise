//! Recent files: a small local list so the home screen can show real thumbnails of what the user
//! opened lately. Stored locally, clearable, never sent anywhere.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::AppError;
use crate::ports::{FileStore, SettingsStore};

/// Most recent files kept.
pub const MAX_RECENT_FILES: usize = 30;

/// One entry in the recent list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecentFile {
    /// Canonical path.
    pub path: PathBuf,
    /// File name for display.
    pub name: String,
    /// When it was last opened, in seconds since the Unix epoch.
    pub opened_at: u64,
}

/// Persistence for the recent list.
pub trait RecentStore: Send + Sync {
    /// Entries, newest first.
    fn load(&self) -> Result<Vec<RecentFile>, AppError>;

    /// Replaces the stored list.
    fn save(&self, files: &[RecentFile]) -> Result<(), AppError>;
}

/// Source of Unix time, so entries can be stamped without the domain knowing clocks.
pub trait UnixClock: Send + Sync {
    /// Seconds since the Unix epoch.
    fn unix_seconds(&self) -> u64;
}

/// The recent files use cases.
pub struct RecentFiles {
    store: Arc<dyn RecentStore>,
    settings: Arc<dyn SettingsStore>,
    files: Arc<dyn FileStore>,
    clock: Arc<dyn UnixClock>,
}

impl RecentFiles {
    /// Wires the use cases to their ports.
    pub fn new(
        store: Arc<dyn RecentStore>,
        settings: Arc<dyn SettingsStore>,
        files: Arc<dyn FileStore>,
        clock: Arc<dyn UnixClock>,
    ) -> Self {
        Self {
            store,
            settings,
            files,
            clock,
        }
    }

    /// Entries that still exist, newest first. Missing files are dropped from the list.
    pub fn list(&self) -> Result<Vec<RecentFile>, AppError> {
        let stored = self.store.load()?;
        let present: Vec<RecentFile> = stored
            .iter()
            .filter(|f| self.files.exists(&f.path))
            .cloned()
            .collect();
        if present.len() != stored.len() {
            self.store.save(&present)?;
        }
        Ok(present)
    }

    /// Records that `path` was opened. Does nothing when the user turned recent files off.
    pub fn add(&self, path: &Path) -> Result<(), AppError> {
        if !self.settings.load()?.remember_recent_files {
            return Ok(());
        }
        let canonical = self.files.canonicalize(path)?;
        let name = canonical
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut list = self.store.load()?;
        list.retain(|f| f.path != canonical);
        list.insert(
            0,
            RecentFile {
                path: canonical,
                name,
                opened_at: self.clock.unix_seconds(),
            },
        );
        list.truncate(MAX_RECENT_FILES);
        self.store.save(&list)
    }

    /// Clears the list.
    pub fn clear(&self) -> Result<(), AppError> {
        self.store.save(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryFiles;
    use domain::settings::Settings;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemStore(Mutex<Vec<RecentFile>>);
    impl RecentStore for MemStore {
        fn load(&self) -> Result<Vec<RecentFile>, AppError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, files: &[RecentFile]) -> Result<(), AppError> {
            *self.0.lock().unwrap() = files.to_vec();
            Ok(())
        }
    }

    struct FixedSettings(bool);
    impl SettingsStore for FixedSettings {
        fn load(&self) -> Result<Settings, AppError> {
            Ok(Settings {
                remember_recent_files: self.0,
                ..Settings::default()
            })
        }
        fn save(&self, _: &Settings) -> Result<(), AppError> {
            Ok(())
        }
    }

    struct Tick(Mutex<u64>);
    impl UnixClock for Tick {
        fn unix_seconds(&self) -> u64 {
            let mut t = self.0.lock().unwrap();
            *t += 1;
            *t
        }
    }

    fn recent(remember: bool, files: Arc<MemoryFiles>) -> RecentFiles {
        RecentFiles::new(
            Arc::new(MemStore::default()),
            Arc::new(FixedSettings(remember)),
            files,
            Arc::new(Tick(Mutex::new(100))),
        )
    }

    fn files() -> Arc<MemoryFiles> {
        let f = Arc::new(MemoryFiles::default());
        for n in ["a", "b", "c"] {
            f.put(&format!("C:\\{n}.pdf"), b"%PDF-");
        }
        f
    }

    #[test]
    fn newest_first_without_duplicates() {
        let r = recent(true, files());
        for n in ["a", "b", "a"] {
            r.add(Path::new(&format!("C:\\{n}.pdf"))).unwrap();
        }
        let names: Vec<String> = r.list().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["a.pdf", "b.pdf"]);
        assert!(r.list().unwrap()[0].opened_at > r.list().unwrap()[1].opened_at);
    }

    #[test]
    fn respects_the_privacy_setting() {
        let r = recent(false, files());
        r.add(Path::new("C:\\a.pdf")).unwrap();
        assert!(r.list().unwrap().is_empty());
    }

    #[test]
    fn caps_the_list_and_clears() {
        let f = Arc::new(MemoryFiles::default());
        let r = recent(true, f.clone());
        for i in 0..(MAX_RECENT_FILES + 5) {
            f.put(&format!("C:\\f{i}.pdf"), b"%PDF-");
            r.add(Path::new(&format!("C:\\f{i}.pdf"))).unwrap();
        }
        assert_eq!(r.list().unwrap().len(), MAX_RECENT_FILES);
        r.clear().unwrap();
        assert!(r.list().unwrap().is_empty());
    }

    #[test]
    fn missing_files_disappear_from_the_list() {
        let f = files();
        let r = recent(true, f.clone());
        r.add(Path::new("C:\\a.pdf")).unwrap();
        r.add(Path::new("C:\\b.pdf")).unwrap();
        f.remove("C:\\a.pdf");
        let names: Vec<String> = r.list().unwrap().into_iter().map(|x| x.name).collect();
        assert_eq!(names, ["b.pdf"]);
    }
}
