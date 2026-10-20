//! Use cases and ports (traits) for Quirewise. Depends only on the domain crate.
//!
//! Engines and infrastructure implement the ports in [`ports`]. Tools implement
//! [`tool::Tool`] against those ports and are registered in a [`registry::ToolRegistry`].
#![forbid(unsafe_code)]

pub mod batch;
pub mod components;
pub mod conformance;
pub mod dto;
pub mod error;
pub mod input;
pub mod inspect;
pub mod ipc;
pub mod naming_service;
pub mod ports;
pub mod query;
pub mod recent;
pub mod registry;
pub mod run_tool;
pub mod runtime;
pub mod signing;
pub mod tool;
pub mod tools;

#[cfg(test)]
mod testing;

pub use error::AppError;
pub use inspect::{DocumentReport, InspectDocument};
pub use query::{DocumentQuery, QueryResult, QueryRunner, QueryService};
pub use recent::{RecentFile, RecentFiles};
pub use registry::ToolRegistry;
pub use run_tool::{RunTool, UnitRequest, UnitRunner};
pub use runtime::{CancellationToken, NoProgress, Password, ProgressEvent, ProgressSink};
pub use tool::{InputDoc, InputKind, InputMode, JobContext, OutputArtifact, Plan, Tool};
