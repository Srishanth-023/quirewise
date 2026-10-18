//! Shared application state managed by Tauri, built once at startup.

use application::ports::{FileStore, SettingsStore};
use application::query::QueryRunner;
use application::{AppError, RecentFiles, ToolRegistry};
use infrastructure::logging::LogGuard;
use infrastructure::{
    AppPaths, AtomicFileStore, EventSink, JobManager, JsonRecentStore, SystemUnixClock,
    TempSession, TomlSettingsStore, WorkerSession, WorkerSupervisor, init_logging,
};
use std::path::PathBuf;
use std::sync::Arc;

/// Everything commands need. No PDF logic lives here: only wiring of ports.
pub struct AppState {
    /// Tool registry, the single source of the tool list.
    pub registry: Arc<ToolRegistry>,
    /// Read-only document queries, answered by the worker.
    pub queries: Arc<dyn QueryRunner>,
    /// Atomic file store.
    pub files: Arc<dyn FileStore>,
    /// Settings persistence.
    pub settings: Arc<dyn SettingsStore>,
    /// Recent files.
    pub recent: RecentFiles,
    /// Background batch jobs.
    pub jobs: JobManager,
    // Held for their Drop behavior: flush logs and wipe the session scratch folder.
    _temp: TempSession,
    _log: LogGuard,
}

/// The worker executable, which sits next to the app executable.
fn worker_exe() -> Result<PathBuf, AppError> {
    let exe = std::env::current_exe().map_err(|e| AppError::from_io("locate the app", &e))?;
    exe.parent()
        .map(|dir| dir.join("quirewise-worker.exe"))
        .ok_or_else(|| AppError::internal("locating the worker"))
}

impl AppState {
    /// Builds the state: paths, logging, settings, worker session and job manager.
    pub fn build(sink: Arc<dyn EventSink>) -> Result<Self, AppError> {
        let paths = AppPaths::from_environment()?;
        std::fs::create_dir_all(&paths.logs_dir)
            .map_err(|e| AppError::from_io("create the log folder", &e))?;
        let log = init_logging(&paths.logs_dir)?;
        let session_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis())
        );
        let temp = TempSession::create(&paths.temp_root, &session_id)?;

        let settings: Arc<dyn SettingsStore> =
            Arc::new(TomlSettingsStore::new(paths.settings_file));
        let loaded = settings.load()?;
        let files: Arc<dyn FileStore> = Arc::new(AtomicFileStore);
        let worker = worker_exe()?;

        let mut registry = ToolRegistry::new();
        application::tools::register_all(&mut registry);

        let cores = std::thread::available_parallelism().map_or(2, usize::from);
        let jobs = JobManager::new(
            Arc::new(WorkerSupervisor::new(worker.clone(), loaded.limits)),
            loaded.heavy_job_slots(cores),
            sink,
        );
        let recent = RecentFiles::new(
            Arc::new(JsonRecentStore::new(paths.recent_db)),
            Arc::clone(&settings),
            Arc::clone(&files),
            Arc::new(SystemUnixClock),
        );
        Ok(Self {
            registry: Arc::new(registry),
            queries: Arc::new(WorkerSession::new(worker, loaded.limits)),
            files,
            settings,
            recent,
            jobs,
            _temp: temp,
            _log: log,
        })
    }
}
