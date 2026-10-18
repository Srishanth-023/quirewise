//! Adapters for Quirewise ports: file store, settings, logging, components, job queue and the
//! worker supervisor. Engines never import this crate and it never imports an engine.
#![forbid(unsafe_code)]

pub mod clock;
pub mod components;
pub mod file_store;
pub mod logging;
pub mod paths;
pub mod queue;
pub mod settings;
pub mod supervisor;

pub use clock::SystemClock;
pub use components::FsComponentStore;
pub use file_store::AtomicFileStore;
pub use logging::{TracingLogger, init_logging};
pub use paths::{AppPaths, TempSession};
pub use queue::{BatchObserver, BatchRequest, JobQueue};
pub use settings::TomlSettingsStore;
pub use supervisor::WorkerSupervisor;
