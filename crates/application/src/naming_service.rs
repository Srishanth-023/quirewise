//! Shared naming service: renders the pattern, sanitizes, and picks a name that collides with
//! neither existing files nor names already reserved by the same job.

use domain::naming::{NameContext, OutputNamingPattern, resolve_collision};
use domain::path_rules::{UncPolicy, check_path_text};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use crate::error::AppError;
use crate::ports::FileStore;

/// Chooses output paths. Share one instance per job so reservations do not collide.
pub struct NamingService {
    files: Arc<dyn FileStore>,
    reserved: Mutex<HashSet<PathBuf>>,
}

impl NamingService {
    /// Creates a service that checks collisions through `files`.
    pub fn new(files: Arc<dyn FileStore>) -> Self {
        Self {
            files,
            reserved: Mutex::new(HashSet::new()),
        }
    }

    /// Returns a free path in `dir` for the rendered pattern, and reserves it.
    pub fn output_path(
        &self,
        dir: &Path,
        pattern: &OutputNamingPattern,
        ctx: &NameContext<'_>,
        extension: &str,
    ) -> Result<PathBuf, AppError> {
        check_path_text(&dir.to_string_lossy(), UncPolicy::UserPicked)?;
        let stem = pattern.render(ctx)?;
        let mut reserved = self.reserved.lock().unwrap_or_else(PoisonError::into_inner);
        let name = resolve_collision(&stem, extension, |candidate| {
            let path = dir.join(candidate);
            self.files.exists(&path) || reserved.contains(&path)
        })?;
        let path = dir.join(name);
        reserved.insert(path.clone());
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryFiles;
    use domain::naming::DateTimeParts;

    fn ctx() -> NameContext<'static> {
        NameContext {
            filename: "a",
            tool: "rotated",
            now: DateTimeParts {
                year: 2026,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
            },
            sequence: None,
            page: None,
            pages: None,
            bookmark: None,
            text: None,
        }
    }

    #[test]
    fn avoids_existing_files_and_earlier_reservations() {
        let files = Arc::new(MemoryFiles::default());
        files.put("C:\\w\\a (rotated).pdf", b"x");
        let svc = NamingService::new(files);
        let p = OutputNamingPattern::default();
        let dir = Path::new("C:\\w");
        let first = svc.output_path(dir, &p, &ctx(), "pdf").unwrap();
        let second = svc.output_path(dir, &p, &ctx(), "pdf").unwrap();
        assert_eq!(first, dir.join("a (rotated) (2).pdf"));
        assert_eq!(second, dir.join("a (rotated) (3).pdf"));
    }

    #[test]
    fn rejects_device_directories() {
        let svc = NamingService::new(Arc::new(MemoryFiles::default()));
        let err = svc
            .output_path(
                Path::new("\\\\.\\PhysicalDrive0"),
                &OutputNamingPattern::default(),
                &ctx(),
                "pdf",
            )
            .unwrap_err();
        assert_eq!(err.code, domain::ErrorCode::PathDenied);
    }
}
