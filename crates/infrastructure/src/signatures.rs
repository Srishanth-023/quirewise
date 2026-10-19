//! Saved signatures: one file for each, sealed by a [`SecretVault`] (DPAPI in the app), so the
//! name, the strokes and the pictures are unreadable to anyone but the person who saved them.

use application::AppError;
use application::ports::{SecretVault, SignatureStore};
use domain::ErrorCode;
use domain::signatures::{MAX_SAVED, NewSavedSignature, SavedSignature};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// File extension of a kept signature.
const EXTENSION: &str = "qwsig";

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// [`SignatureStore`] over a folder of sealed files.
#[derive(Clone)]
pub struct FsSignatureStore {
    dir: PathBuf,
    vault: Arc<dyn SecretVault>,
}

impl FsSignatureStore {
    /// Keeps signatures in `dir`, sealed by `vault`.
    pub fn new(dir: PathBuf, vault: Arc<dyn SecretVault>) -> Self {
        Self { dir, vault }
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.{EXTENSION}"))
    }

    fn read(&self, path: &Path) -> Result<SavedSignature, AppError> {
        let sealed = fs::read(path).map_err(|e| AppError::from_io("read a saved signature", &e))?;
        let plain = self.vault.unprotect(&sealed)?;
        serde_json::from_slice(&plain)
            .map_err(|_| AppError::new(ErrorCode::Internal, "A saved signature is damaged."))
    }

    fn files(&self) -> Vec<(PathBuf, std::time::SystemTime)> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut files: Vec<(PathBuf, std::time::SystemTime)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == EXTENSION))
            .map(|e| {
                let modified = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                (e.path(), modified)
            })
            .collect();
        files.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        files
    }
}

/// An identifier nobody can guess or collide with: a hash of the time, the process and a counter.
fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(COUNTER.fetch_add(1, Ordering::SeqCst).to_le_bytes());
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit())
}

impl SignatureStore for FsSignatureStore {
    fn list(&self) -> Result<Vec<SavedSignature>, AppError> {
        let mut out = Vec::new();
        for (path, _) in self.files() {
            // A file that cannot be opened (another user's, or damaged) is left alone, not shown.
            if let Ok(signature) = self.read(&path) {
                out.push(signature);
            }
        }
        Ok(out)
    }

    fn save(&self, new: &NewSavedSignature) -> Result<SavedSignature, AppError> {
        new.validate()?;
        if self.files().len() >= MAX_SAVED {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                format!("You can keep at most {MAX_SAVED} signatures. Delete one first."),
            ));
        }
        let saved = SavedSignature {
            id: new_id(),
            name: new.name.trim().to_owned(),
            kind: new.kind,
            aspect: new.aspect,
            source: new.source.clone(),
        };
        let json =
            serde_json::to_vec(&saved).map_err(|_| AppError::internal("saving the signature"))?;
        let sealed = self.vault.protect(&json)?;
        fs::create_dir_all(&self.dir)
            .map_err(|e| AppError::from_io("create the signatures folder", &e))?;
        let target = self.file(&saved.id);
        let temp = self.dir.join(format!("{}.tmp", saved.id));
        fs::write(&temp, sealed).map_err(|e| AppError::from_io("save the signature", &e))?;
        fs::rename(&temp, &target).map_err(|e| {
            let _ = fs::remove_file(&temp);
            AppError::from_io("save the signature", &e)
        })?;
        Ok(saved)
    }

    fn delete(&self, id: &str) -> Result<(), AppError> {
        // Only identifiers this store made: nothing else can name a path outside the folder.
        if !valid_id(id) {
            return Ok(());
        }
        match fs::remove_file(self.file(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::from_io("delete the signature", &e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::signatures::SavedKind;
    use domain::tools::editor::{SignatureSource, TextStyle};

    /// A vault that only flips the bytes, so the tests can see that files are not plain.
    struct Flip;
    impl SecretVault for Flip {
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, AppError> {
            Ok(plain.iter().map(|b| !b).collect())
        }
        fn unprotect(&self, sealed: &[u8]) -> Result<Vec<u8>, AppError> {
            Ok(sealed.iter().map(|b| !b).collect())
        }
    }

    fn store(dir: &Path) -> FsSignatureStore {
        FsSignatureStore::new(dir.join("sigs"), Arc::new(Flip))
    }

    fn new(name: &str) -> NewSavedSignature {
        NewSavedSignature {
            name: name.to_owned(),
            kind: SavedKind::Signature,
            aspect: 3.0,
            source: SignatureSource::Typed {
                text: "Ada Lovelace".to_owned(),
                style: TextStyle::default(),
            },
        }
    }

    #[test]
    fn saved_signatures_are_listed_in_order_and_sealed_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        assert!(s.list().unwrap().is_empty());
        let first = s.save(&new("First")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = s.save(&new("  Second  ")).unwrap();
        let listed = s.list().unwrap();
        assert_eq!(
            listed.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert_eq!(listed[0], first);
        assert_ne!(first.id, second.id);
        // The file does not hold the text in the clear.
        let raw = fs::read(dir.path().join("sigs").join(format!("{}.qwsig", first.id))).unwrap();
        assert!(!raw.windows(5).any(|w| w == b"Ada L"));
        assert!(!String::from_utf8_lossy(&raw).contains("First"));
    }

    #[test]
    fn deleting_removes_one_and_ignores_unknown_or_hostile_ids() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        let a = s.save(&new("A")).unwrap();
        let b = s.save(&new("B")).unwrap();
        s.delete(&a.id).unwrap();
        assert_eq!(s.list().unwrap(), vec![b.clone()]);
        s.delete(&a.id).unwrap();
        // An id that is not one of ours never reaches the file system.
        let outside = dir.path().join("keep.qwsig");
        fs::write(&outside, b"x").unwrap();
        for hostile in ["..\\keep", "../keep", "keep", "", "C:\\Windows"] {
            s.delete(hostile).unwrap();
        }
        assert!(outside.is_file());
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn invalid_signatures_and_too_many_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        assert!(s.save(&new("")).is_err());
        for i in 0..MAX_SAVED {
            s.save(&new(&format!("s{i}"))).unwrap();
        }
        let err = s.save(&new("one more")).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
    }

    #[test]
    fn a_file_that_cannot_be_opened_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.save(&new("Good")).unwrap();
        fs::write(
            dir.path().join("sigs").join("0123456789abcdef.qwsig"),
            b"junk bytes",
        )
        .unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
    }
}
