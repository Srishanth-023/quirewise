//! Reads installed components from `components\<id>\<version>\component.json`.
//!
//! Downloading, signature verification and installation arrive with the component manager in
//! Phase 6; this adapter only reports what is already installed and verified.

use application::AppError;
use application::ports::ComponentStore;
use domain::components::ComponentEntry;
use std::fs;
use std::path::PathBuf;

/// File name of the record written next to an installed component.
pub const RECORD_FILE: &str = "component.json";

/// Component store over the components folder.
#[derive(Debug, Clone)]
pub struct FsComponentStore {
    root: PathBuf,
}

impl FsComponentStore {
    /// Creates a store over `root` (the `components` folder).
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl ComponentStore for FsComponentStore {
    fn installed(&self) -> Result<Vec<ComponentEntry>, AppError> {
        let Ok(ids) = fs::read_dir(&self.root) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for id_dir in ids.flatten().filter(|e| e.path().is_dir()) {
            let Ok(versions) = fs::read_dir(id_dir.path()) else {
                continue;
            };
            for version_dir in versions.flatten().filter(|e| e.path().is_dir()) {
                let record = version_dir.path().join(RECORD_FILE);
                // A missing or invalid record means the install is incomplete; skip it.
                let Ok(bytes) = fs::read(&record) else {
                    continue;
                };
                if let Ok(entry) = serde_json::from_slice::<ComponentEntry>(&bytes)
                    && entry.validate().is_ok()
                {
                    found.push(entry);
                }
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::Bytes;
    use domain::components::Arch;
    use tempfile::tempdir;

    fn entry(id: &str) -> ComponentEntry {
        ComponentEntry {
            id: id.to_owned(),
            version: "1.0".to_owned(),
            arch: Arch::Any,
            url: "https://example.invalid/x.zip".to_owned(),
            size: Bytes::from_mb(1),
            sha256: "c".repeat(64),
        }
    }

    #[test]
    fn missing_folder_means_nothing_installed() {
        let store = FsComponentStore::new(PathBuf::from(r"Z:\definitely\missing"));
        assert!(store.installed().unwrap().is_empty());
    }

    #[test]
    fn lists_valid_records_and_skips_broken_ones() {
        let dir = tempdir().unwrap();
        let good = dir.path().join("tessdata-eng").join("1.0");
        fs::create_dir_all(&good).unwrap();
        fs::write(
            good.join(RECORD_FILE),
            serde_json::to_vec(&entry("tessdata-eng")).unwrap(),
        )
        .unwrap();
        let broken = dir.path().join("llama").join("2.0");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join(RECORD_FILE), b"{not json").unwrap();
        let incomplete = dir.path().join("verapdf").join("1.0");
        fs::create_dir_all(&incomplete).unwrap();

        let list = FsComponentStore::new(dir.path().to_path_buf())
            .installed()
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "tessdata-eng");
    }
}
