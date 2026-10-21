//! Typed commands exposed to the UI. Each maps a DTO to an application use case; none contains
//! PDF logic. Argument names are camelCase on the JavaScript side.
//!
//! The submodules are public because `generate_handler!` needs the full command paths.

pub mod ai;
pub mod components;
pub mod documents;
pub mod editor;
pub mod explorer;
pub mod jobs;
pub mod recent;
pub mod recipes;
pub mod scan;
pub mod settings;
pub mod system;
