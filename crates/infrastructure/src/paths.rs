//! Where Quirewise keeps its files, including portable mode and the per-session scratch folder.

use application::AppError;
use std::fs;
use std::path::{Path, PathBuf};

/// Name of the marker file that switches on portable mode when it sits next to the exe.
pub const PORTABLE_FLAG: &str = "portable.flag";

/// Resolved locations for settings, logs, components and scratch space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// `settings.toml`.
    pub settings_file: PathBuf,
    /// Logs folder (daily files, 14-day retention).
    pub logs_dir: PathBuf,
    /// Installed on-demand components.
    pub components_dir: PathBuf,
    /// Local-only crash reports and minidumps.
    pub crashes_dir: PathBuf,
    /// Recent-files database.
    pub recent_db: PathBuf,
    /// Saved recipes.
    pub recipes_file: PathBuf,
    /// Saved signatures, each one encrypted for the current user.
    pub signatures_dir: PathBuf,
    /// Parent of per-session scratch folders.
    pub temp_root: PathBuf,
    /// True when running in portable mode.
    pub portable: bool,
}

impl AppPaths {
    /// Resolves paths from an environment lookup and the exe folder.
    ///
    /// Installed: `%APPDATA%\Quirewise\settings.toml`, `%LOCALAPPDATA%\Quirewise\...`,
    /// `%TEMP%\Quirewise`. Portable (flag file next to the exe): everything under `.\data\`.
    pub fn resolve(
        env: &dyn Fn(&str) -> Option<String>,
        exe_dir: &Path,
        portable: bool,
    ) -> Result<Self, AppError> {
        if portable {
            let data = exe_dir.join("data");
            return Ok(Self {
                settings_file: data.join("settings.toml"),
                logs_dir: data.join("logs"),
                components_dir: data.join("components"),
                crashes_dir: data.join("crashes"),
                recent_db: data.join("recent.db"),
                recipes_file: data.join("recipes.json"),
                signatures_dir: data.join("signatures"),
                temp_root: data.join("temp"),
                portable: true,
            });
        }
        let var = |name: &str| {
            env(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| AppError::internal(&format!("finding the {name} folder")))
        };
        let roaming = var("APPDATA")?.join("Quirewise");
        let local = var("LOCALAPPDATA")?.join("Quirewise");
        Ok(Self {
            settings_file: roaming.join("settings.toml"),
            logs_dir: local.join("logs"),
            components_dir: local.join("components"),
            crashes_dir: local.join("crashes"),
            recent_db: local.join("recent.db"),
            recipes_file: roaming.join("recipes.json"),
            signatures_dir: local.join("signatures"),
            temp_root: var("TEMP")?.join("Quirewise"),
            portable: false,
        })
    }

    /// Resolves from the real environment and detects portable mode from the exe folder.
    pub fn from_environment() -> Result<Self, AppError> {
        let exe = std::env::current_exe().map_err(|e| AppError::from_io("locate the app", &e))?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| AppError::internal("locating the app folder"))?
            .to_path_buf();
        let portable = exe_dir.join(PORTABLE_FLAG).is_file();
        Self::resolve(&|name| std::env::var(name).ok(), &exe_dir, portable)
    }
}

/// A scratch folder for one app session, deleted when dropped.
#[derive(Debug)]
pub struct TempSession {
    path: PathBuf,
}

impl TempSession {
    /// Creates `<root>\<session_id>` and wipes folders left behind by earlier sessions.
    pub fn create(root: &Path, session_id: &str) -> Result<Self, AppError> {
        sweep_stale(root, session_id);
        let path = root.join(session_id);
        fs::create_dir_all(&path)
            .map_err(|e| AppError::from_io("create the scratch folder", &e))?;
        Ok(Self { path })
    }

    /// The scratch folder.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempSession {
    fn drop(&mut self) {
        // Best effort; anything left is swept on the next start.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Deletes every child folder of `root` except `keep`.
pub fn sweep_stale(root: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy() != keep {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn env(name: &str) -> Option<String> {
        match name {
            "APPDATA" => Some(r"C:\Users\me\AppData\Roaming".to_owned()),
            "LOCALAPPDATA" => Some(r"C:\Users\me\AppData\Local".to_owned()),
            "TEMP" => Some(r"C:\Users\me\AppData\Local\Temp".to_owned()),
            _ => None,
        }
    }

    #[test]
    fn installed_layout_follows_the_spec() {
        let p = AppPaths::resolve(&env, Path::new(r"C:\Apps\Quirewise"), false).unwrap();
        assert_eq!(
            p.settings_file,
            Path::new(r"C:\Users\me\AppData\Roaming\Quirewise\settings.toml")
        );
        assert_eq!(
            p.logs_dir,
            Path::new(r"C:\Users\me\AppData\Local\Quirewise\logs")
        );
        assert_eq!(
            p.temp_root,
            Path::new(r"C:\Users\me\AppData\Local\Temp\Quirewise")
        );
        assert!(!p.portable);
    }

    #[test]
    fn portable_mode_keeps_everything_under_data() {
        let p = AppPaths::resolve(&|_| None, Path::new(r"D:\Portable\Quirewise"), true).unwrap();
        assert!(p.portable);
        for path in [
            &p.settings_file,
            &p.logs_dir,
            &p.components_dir,
            &p.crashes_dir,
            &p.recent_db,
            &p.temp_root,
        ] {
            assert!(path.starts_with(r"D:\Portable\Quirewise\data"), "{path:?}");
        }
    }

    #[test]
    fn missing_environment_is_an_error_not_a_panic() {
        assert!(AppPaths::resolve(&|_| None, Path::new("x"), false).is_err());
        assert!(AppPaths::resolve(&|_| Some(String::new()), Path::new("x"), false).is_err());
    }

    #[test]
    fn session_folder_is_created_swept_and_removed() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("old-session")).unwrap();
        fs::write(root.path().join("old-session").join("x.tmp"), b"x").unwrap();
        let session_path;
        {
            let s = TempSession::create(root.path(), "s1").unwrap();
            session_path = s.path().to_path_buf();
            assert!(session_path.is_dir());
            assert!(
                !root.path().join("old-session").exists(),
                "stale session wiped"
            );
        }
        assert!(!session_path.exists(), "session wiped on drop");
    }
}
