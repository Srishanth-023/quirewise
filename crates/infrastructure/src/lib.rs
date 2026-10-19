//! Adapters for Quirewise ports: file store, settings, logging, components, job queue and the
//! worker supervisor. Engines never import this crate and it never imports an engine.
#![forbid(unsafe_code)]

pub mod clock;
pub mod components;
pub mod download;
pub mod file_store;
pub mod jobs;
pub mod logging;
pub mod native;
pub mod paths;
pub mod queue;
pub mod recent;
pub mod session;
pub mod settings;
pub mod supervisor;

pub use clock::SystemClock;
pub use components::{ComponentManager, ComponentTrust, FsComponentStore};
pub use download::HttpsDownloader;
pub use file_store::AtomicFileStore;
pub use jobs::{EventSink, JobManager};
pub use logging::{TracingLogger, init_logging};
pub use native::NativePaths;
pub use paths::{AppPaths, TempSession};
pub use queue::{BatchObserver, BatchRequest, JobQueue};
pub use recent::{JsonRecentStore, SystemUnixClock};
pub use session::WorkerSession;
pub use settings::TomlSettingsStore;
pub use supervisor::WorkerSupervisor;
