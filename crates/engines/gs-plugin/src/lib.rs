//! Ghostscript adapter, an opt-in plugin: a second way to make a PDF/A file for documents that the
//! built-in normalizer cannot fix.
//!
//! Ghostscript is licensed AGPL-3.0, so it is never part of the default build and never linked:
//! with the Cargo feature `agpl-ghostscript` this crate runs a Ghostscript that the person
//! installed, as a separate process, and nothing else. See `THIRD_PARTY_NOTICES.md`.
#![forbid(unsafe_code)]

#[cfg(feature = "agpl-ghostscript")]
mod plugin;

#[cfg(feature = "agpl-ghostscript")]
pub use plugin::Ghostscript;
