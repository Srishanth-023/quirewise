//! Adapters for Quirewise ports: file store, settings, logging, components, job queue and the
//! worker supervisor. Engines never import this crate and it never imports an engine.
// One module (`dpapi`) calls Windows and allows unsafe code for that; everything else may not.
#![deny(unsafe_code)]

pub mod clock;
pub mod components;
pub mod download;
#[cfg(windows)]
pub mod dpapi;
pub mod file_store;
pub mod jobs;
pub mod logging;
pub mod native;
pub mod paths;
pub mod queue;
pub mod recent;
pub mod recipes;
pub mod session;
pub mod settings;
pub mod signatures;
pub mod supervisor;
pub mod web;

pub use clock::SystemClock;
pub use components::{ComponentManager, ComponentTrust, FsComponentStore, default_trust};
pub use download::HttpsDownloader;
pub use file_store::AtomicFileStore;
pub use jobs::{EventSink, JobManager};
pub use logging::{TracingLogger, init_logging};
pub use native::NativePaths;
pub use paths::{AppPaths, TempSession};
pub use queue::{BatchObserver, BatchRequest, JobQueue};
pub use recent::{JsonRecentStore, SystemUnixClock};
pub use recipes::{JsonRecipeStore, folder_inputs};
pub use session::WorkerSession;
pub use settings::TomlSettingsStore;
pub use supervisor::WorkerSupervisor;
pub use web::HttpWeb;
