//! `settings.toml` with versioned migrations.

use application::AppError;
use application::ports::SettingsStore;
use domain::ErrorCode;
use domain::settings::{CURRENT_SETTINGS_VERSION, Settings};
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::file_store::replace_own_file;

/// TOML-backed settings store.
#[derive(Debug, Clone)]
pub struct TomlSettingsStore {
    path: PathBuf,
}

impl TomlSettingsStore {
    /// Creates a store for the file at `path`. The file need not exist yet.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

/// Applies one migration step to a raw table and returns the version it produced.
///
/// Add an arm and bump `CURRENT_SETTINGS_VERSION` whenever the shape changes.
fn migrate_step(table: &mut toml::Table, from: u32) -> u32 {
    match from {
        // Version 0 files predate the `version` key.
        0 => {
            table.insert("version".to_owned(), toml::Value::Integer(1));
            1
        }
        other => other + 1,
    }
}

fn read_version(table: &toml::Table) -> u32 {
    table
        .get("version")
        .and_then(toml::Value::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
}

/// Why settings text could not be used.
#[derive(Debug)]
enum ParseFailure {
    /// Written by a newer app version; the file must be left untouched.
    TooNew,
    /// Malformed or invalid; the file can be set aside.
    Unreadable,
}

impl ParseFailure {
    fn into_app_error(self) -> AppError {
        match self {
            Self::TooNew => AppError::new(
                ErrorCode::OptionInvalid,
                "The settings file was written by a newer Quirewise. Update the app or reset your settings.",
            ),
            Self::Unreadable => AppError::new(
                ErrorCode::OptionInvalid,
                "The settings file could not be read. It was set aside and defaults are in use.",
            ),
        }
    }
}

/// Parses settings text, migrating older versions.
fn parse_settings(text: &str) -> Result<Settings, ParseFailure> {
    let mut table: toml::Table = text.parse().map_err(|_| ParseFailure::Unreadable)?;
    let mut version = read_version(&table);
    if version > CURRENT_SETTINGS_VERSION {
        return Err(ParseFailure::TooNew);
    }
    while version < CURRENT_SETTINGS_VERSION {
        version = migrate_step(&mut table, version);
    }
    let settings: Settings = table.try_into().map_err(|_| ParseFailure::Unreadable)?;
    settings.validate().map_err(|_| ParseFailure::Unreadable)?;
    Ok(settings)
}

impl SettingsStore for TomlSettingsStore {
    fn load(&self) -> Result<Settings, AppError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(AppError::from_io("read the settings", &e)),
        };
        match parse_settings(&text) {
            Ok(s) => Ok(s),
            Err(ParseFailure::TooNew) => Err(ParseFailure::TooNew.into_app_error()),
            Err(ParseFailure::Unreadable) => {
                // Keep the broken file for inspection instead of silently losing it.
                let broken = self.path.with_extension("toml.broken");
                let _ = fs::remove_file(&broken);
                let _ = fs::rename(&self.path, &broken);
                Ok(Settings::default())
            }
        }
    }

    fn save(&self, settings: &Settings) -> Result<(), AppError> {
        settings.validate()?;
        let text =
            toml::to_string_pretty(settings).map_err(|_| AppError::internal("saving settings"))?;
        replace_own_file(&self.path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::settings::ThemePreference;
    use tempfile::tempdir;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempdir().unwrap();
        let store = TomlSettingsStore::new(dir.path().join("settings.toml"));
        assert_eq!(store.load().unwrap(), Settings::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let store = TomlSettingsStore::new(dir.path().join("nested").join("settings.toml"));
        let settings = Settings {
            theme: ThemePreference::Dark,
            max_heavy_jobs: Some(2),
            ..Settings::default()
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap(), settings);
    }

    #[test]
    fn default_settings_serialize_without_none_errors() {
        let dir = tempdir().unwrap();
        let store = TomlSettingsStore::new(dir.path().join("s.toml"));
        store.save(&Settings::default()).unwrap();
        assert_eq!(store.load().unwrap(), Settings::default());
    }

    #[test]
    fn version_zero_files_are_migrated() {
        let s = parse_settings("theme = \"light\"\n").unwrap();
        assert_eq!(s.version, CURRENT_SETTINGS_VERSION);
        assert_eq!(s.theme, ThemePreference::Light);
    }

    #[test]
    fn newer_versions_are_refused() {
        assert!(matches!(
            parse_settings("version = 99\n"),
            Err(ParseFailure::TooNew)
        ));
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(parse_settings("max_heavy_jobs = 9\n").is_err());
        assert!(parse_settings("unknown_key = 1\n").is_err());
        assert!(parse_settings("not toml [[[").is_err());
    }

    #[test]
    fn broken_file_is_set_aside_and_defaults_used() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "theme = [oops").unwrap();
        let store = TomlSettingsStore::new(path.clone());
        assert_eq!(store.load().unwrap(), Settings::default());
        assert!(!path.exists());
        assert!(dir.path().join("settings.toml.broken").exists());
    }

    #[test]
    fn newer_file_is_not_clobbered() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "version = 99\n").unwrap();
        let store = TomlSettingsStore::new(path.clone());
        assert!(store.load().is_err());
        assert!(path.exists());
    }
}
