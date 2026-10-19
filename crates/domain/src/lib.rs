//! Pure types and rules for Quirewise: page ranges, tool options, jobs, naming and error codes.
//!
//! This crate performs no I/O and depends on nothing project-internal.
#![forbid(unsafe_code)]

pub mod components;
pub mod error;
pub mod fonts;
pub mod job;
pub mod layout;
pub mod limits;
pub mod naming;
pub mod page_range;
pub mod path_rules;
pub mod planners;
pub mod secret;
pub mod settings;
pub mod stamp;
pub mod tools;
pub mod units;

pub use error::{DomainError, ErrorCode};
pub use page_range::{Duplicates, PageRange, PageSelection};
pub use tools::{ToolId, ToolOptions};
pub use units::{Bytes, Dpi, Millimeters, PageIndex, PageNumber, Percent};
