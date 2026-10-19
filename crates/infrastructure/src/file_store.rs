//! Atomic, never-overwriting file store (AGENTS.md section 7).

use application::AppError;
use application::ports::FileStore;
use domain::ErrorCode;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// File store over `std::fs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct AtomicFileStore;

/// Removes the temp file on drop unless the output was published.
struct TempGuard<'a> {
    path: &'a Path,
    armed: bool,
}

impl Drop for TempGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            // Best effort: a leftover temp is wiped with the session folder or next start.
            let _ = fs::remove_file(self.path);
        }
    }
}

/// Strips the `\\?\` verbatim prefix that `canonicalize` adds, so paths stay readable and match
/// what the user picked. Device and volume forms are left alone.
pub fn simplify_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    path
}

fn temp_name(dest: &Path) -> String {
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(".{stem}.{}.{nanos}.qwtmp", std::process::id())
}

impl FileStore for AtomicFileStore {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, AppError> {
        fs::canonicalize(path)
            .map(simplify_verbatim)
            .map_err(|e| AppError::from_io("open the file", &e))
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn len(&self, path: &Path) -> Result<u64, AppError> {
        fs::metadata(path)
            .map(|m| m.len())
            .map_err(|e| AppError::from_io("read the file size", &e))
    }

    fn read_head(&self, path: &Path, max: usize) -> Result<Vec<u8>, AppError> {
        let file = File::open(path).map_err(|e| AppError::from_io("open the file", &e))?;
        let mut head = Vec::new();
        file.take(max as u64)
            .read_to_end(&mut head)
            .map_err(|e| AppError::from_io("read the file", &e))?;
        Ok(head)
    }

    fn remove_file(&self, path: &Path) {
        // Best effort: leftovers are wiped with the session scratch folder.
        let _ = fs::remove_file(path);
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Result<(), AppError> {
        fs::copy(from, to)
            .map(|_| ())
            .map_err(|e| AppError::from_io("copy the file", &e))
    }

    fn modified_unix(&self, path: &Path) -> Option<u64> {
        fs::metadata(path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    }

    fn publish(
        &self,
        dest: &Path,
        produce: &mut dyn FnMut(&Path) -> Result<(), AppError>,
        verify: &dyn Fn(&Path) -> Result<(), AppError>,
    ) -> Result<PathBuf, AppError> {
        let denied = || {
            AppError::new(
                ErrorCode::PathDenied,
                "An output file with this name already exists. Quirewise never overwrites files; run again to get a new name.",
            )
        };
        if dest.exists() {
            return Err(denied());
        }
        let dir = dest
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .ok_or_else(|| AppError::internal("choosing the output folder"))?;
        // A chosen output folder may not exist yet (`qw run --out`, a new folder in the app).
        fs::create_dir_all(dir).map_err(|e| AppError::from_io("create the output folder", &e))?;
        let temp = dir.join(temp_name(dest));
        let mut guard = TempGuard {
            path: &temp,
            armed: true,
        };

        produce(&temp)?;
        // The engine wrote the file; make sure it reached the disk before it is verified.
        // Windows needs write access to flush file buffers.
        OpenOptions::new()
            .write(true)
            .open(&temp)
            .and_then(|f| f.sync_all())
            .map_err(|e| AppError::from_io("save the output", &e))?;

        verify(&temp)?;

        // A hard link fails when `dest` exists, so a racing writer is never overwritten.
        match fs::hard_link(&temp, dest) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(denied()),
            Err(_) => {
                // The file system has no hard links; fall back to rename after re-checking.
                if dest.exists() {
                    return Err(denied());
                }
                fs::rename(&temp, dest).map_err(|e| AppError::from_io("save the output", &e))?;
                guard.armed = false;
                return Ok(dest.to_path_buf());
            }
        }
        // `guard` still armed: it removes the now-redundant temp name.
        Ok(dest.to_path_buf())
    }

    fn write_atomic(
        &self,
        dest: &Path,
        write: &mut dyn FnMut(&mut dyn Write) -> io::Result<()>,
        verify: &dyn Fn(&Path) -> Result<(), AppError>,
    ) -> Result<PathBuf, AppError> {
        self.publish(
            dest,
            &mut |temp| {
                let file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(temp)
                    .map_err(|e| AppError::from_io("create the output file", &e))?;
                let mut writer = BufWriter::new(file);
                write(&mut writer).map_err(|e| AppError::from_io("write the output", &e))?;
                writer
                    .flush()
                    .map_err(|e| AppError::from_io("write the output", &e))
            },
            verify,
        )
    }
}

/// Replaces a file that Quirewise itself owns (settings, recent-files database) atomically.
///
/// Unlike [`FileStore::write_atomic`] this overwrites on purpose; never use it for user outputs.
pub fn replace_own_file(dest: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let dir = dest
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| AppError::internal("saving settings"))?;
    fs::create_dir_all(dir).map_err(|e| AppError::from_io("create the settings folder", &e))?;
    let temp = dir.join(temp_name(dest));
    let mut guard = TempGuard {
        path: &temp,
        armed: true,
    };
    let mut file = File::create(&temp).map_err(|e| AppError::from_io("save the file", &e))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| AppError::from_io("save the file", &e))?;
    drop(file);
    fs::rename(&temp, dest).map_err(|e| AppError::from_io("save the file", &e))?;
    guard.armed = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn leftovers(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".qwtmp"))
            .collect()
    }

    #[test]
    fn writes_verifies_and_publishes_without_leftovers() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.pdf");
        let verified = std::cell::Cell::new(false);
        let path = AtomicFileStore
            .write_atomic(&dest, &mut |w| w.write_all(b"%PDF-1.7 hello"), &|temp| {
                assert!(temp.exists(), "verify sees the temp file");
                assert!(!dest.exists(), "dest not published before verify");
                verified.set(true);
                Ok(())
            })
            .unwrap();
        assert!(verified.get());
        assert_eq!(fs::read(&path).unwrap(), b"%PDF-1.7 hello");
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn failed_verification_removes_the_temp_and_publishes_nothing() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.pdf");
        let err = AtomicFileStore
            .write_atomic(&dest, &mut |w| w.write_all(b"data"), &|_| {
                Err(AppError::new(ErrorCode::OutputVerifyFailed, "bad"))
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::OutputVerifyFailed);
        assert!(!dest.exists());
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn failed_write_cleans_up() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.pdf");
        let err = AtomicFileStore
            .write_atomic(&dest, &mut |_| Err(io::Error::other("disk full")), &|_| {
                Ok(())
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(!err.message.contains("disk full"));
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn never_overwrites_an_existing_file() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("keep.pdf");
        fs::write(&dest, b"original").unwrap();
        let err = AtomicFileStore
            .write_atomic(&dest, &mut |w| w.write_all(b"new"), &|_| Ok(()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PathDenied);
        assert_eq!(fs::read(&dest).unwrap(), b"original");
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn creates_a_missing_output_folder() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("new").join("deeper").join("out.pdf");
        AtomicFileStore
            .write_atomic(&dest, &mut |w| w.write_all(b"data"), &|_| Ok(()))
            .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"data");
        assert!(leftovers(dest.parent().unwrap()).is_empty());
    }

    #[test]
    fn read_head_and_len_and_canonicalize() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("a.bin");
        fs::write(&file, b"0123456789").unwrap();
        assert_eq!(AtomicFileStore.read_head(&file, 4).unwrap(), b"0123");
        assert_eq!(AtomicFileStore.len(&file).unwrap(), 10);
        let canon = AtomicFileStore.canonicalize(&file).unwrap();
        assert!(!canon.to_string_lossy().starts_with(r"\\?\"));
        assert!(
            AtomicFileStore
                .canonicalize(&dir.path().join("none"))
                .is_err()
        );
    }

    #[test]
    fn verbatim_prefixes_are_simplified() {
        assert_eq!(
            simplify_verbatim(PathBuf::from(r"\\?\C:\x\y.pdf")),
            PathBuf::from(r"C:\x\y.pdf")
        );
        assert_eq!(
            simplify_verbatim(PathBuf::from(r"\\?\UNC\srv\share\a.pdf")),
            PathBuf::from(r"\\srv\share\a.pdf")
        );
        assert_eq!(
            simplify_verbatim(PathBuf::from(r"\\?\Volume{1}\a")),
            PathBuf::from(r"\\?\Volume{1}\a")
        );
    }

    #[test]
    fn replace_own_file_overwrites_atomically() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("settings.toml");
        replace_own_file(&file, b"a = 1").unwrap();
        replace_own_file(&file, b"a = 2").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"a = 2");
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn publish_lets_an_engine_write_to_a_temp_path_then_publishes_it() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("engine out.pdf");
        let seen = std::cell::RefCell::new(None);
        let path = AtomicFileStore
            .publish(
                &dest,
                &mut |temp| {
                    assert_eq!(
                        temp.parent(),
                        dest.parent(),
                        "temp lives next to the destination"
                    );
                    assert!(!dest.exists());
                    fs::write(temp, b"engine output").map_err(|e| AppError::from_io("write", &e))
                },
                &|temp| {
                    *seen.borrow_mut() = Some(fs::read(temp).unwrap());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"engine output");
        assert_eq!(seen.borrow().as_deref(), Some(&b"engine output"[..]));
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn publish_failures_leave_no_output_and_no_temp() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("a.pdf");
        let failing = AtomicFileStore.publish(
            &dest,
            &mut |temp| {
                fs::write(temp, b"partial").unwrap();
                Err(AppError::new(ErrorCode::PdfDamaged, "engine failed"))
            },
            &|_| Ok(()),
        );
        assert_eq!(failing.unwrap_err().code, ErrorCode::PdfDamaged);
        assert!(!dest.exists());
        assert!(leftovers(dir.path()).is_empty());

        // An engine that reports success without writing anything fails verification of a missing file.
        let silent = AtomicFileStore.publish(&dest, &mut |_| Ok(()), &|_| Ok(()));
        assert!(silent.is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn publish_never_overwrites() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("keep.pdf");
        fs::write(&dest, b"original").unwrap();
        let result = AtomicFileStore.publish(
            &dest,
            &mut |t| fs::write(t, b"new").map_err(|e| AppError::from_io("w", &e)),
            &|_| Ok(()),
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::PathDenied);
        assert_eq!(fs::read(&dest).unwrap(), b"original");
    }

    #[test]
    fn remove_file_and_modified_time() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("x.tmp");
        fs::write(&file, b"x").unwrap();
        assert!(AtomicFileStore.modified_unix(&file).unwrap() > 1_767_225_600);
        AtomicFileStore.remove_file(&file);
        assert!(!file.exists());
        AtomicFileStore.remove_file(&file);
        assert_eq!(AtomicFileStore.modified_unix(&file), None);
    }
}
