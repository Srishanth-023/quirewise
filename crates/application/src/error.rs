//! The single error type at the application boundary: a stable code plus a message that says
//! what happened and what to do next. Never carries stack traces or document content.

use domain::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use std::io;
use thiserror::Error;

/// Error returned by use cases and ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Error)]
#[error("{code}: {message}")]
pub struct AppError {
    /// Stable code from the catalog.
    pub code: ErrorCode,
    /// User-facing message: what happened and what to do next.
    pub message: String,
}

impl AppError {
    /// Creates an error with a code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Puts `context` in front of the message, so a chained run says which step failed. A
    /// cancellation keeps its own message.
    pub fn with_context(mut self, context: &str) -> Self {
        if self.code != ErrorCode::JobCancelled {
            self.message = format!("{context}: {}", self.message);
        }
        self
    }

    /// The job was cancelled by the user.
    pub fn cancelled() -> Self {
        Self::new(
            ErrorCode::JobCancelled,
            "The job was cancelled. No output was written and your files are unchanged.",
        )
    }

    /// An unexpected failure. The detail is logged by the caller, not shown raw.
    pub fn internal(what: &str) -> Self {
        Self::new(
            ErrorCode::Internal,
            format!(
                "Something went wrong while {what}. Try again; if it keeps happening, check the log folder."
            ),
        )
    }

    /// Maps an I/O failure to a catalog code without exposing raw OS text.
    pub fn from_io(action: &str, err: &io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::NotFound => Self::new(
                ErrorCode::PathDenied,
                format!(
                    "Could not {action}: the file or folder was not found. Check that it still exists."
                ),
            ),
            io::ErrorKind::PermissionDenied => Self::new(
                ErrorCode::PathDenied,
                format!(
                    "Could not {action}: access was denied. Close other programs using it or choose another folder."
                ),
            ),
            _ => Self::internal(action),
        }
    }
}

impl From<DomainError> for AppError {
    fn from(err: DomainError) -> Self {
        Self::new(err.code(), err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_errors_keep_their_code_and_message() {
        let e: AppError = DomainError::option("dpi", "too high").into();
        assert_eq!(e.code, ErrorCode::OptionInvalid);
        assert!(e.message.contains("dpi"));
    }

    #[test]
    fn io_errors_do_not_leak_os_text() {
        let raw = io::Error::new(
            io::ErrorKind::PermissionDenied,
            "os error 5: C:\\secret\\x.pdf",
        );
        let e = AppError::from_io("write the output", &raw);
        assert_eq!(e.code, ErrorCode::PathDenied);
        assert!(!e.message.contains("secret"));
        let other = AppError::from_io("write the output", &io::Error::other("boom"));
        assert_eq!(other.code, ErrorCode::Internal);
        assert!(!other.message.contains("boom"));
    }

    #[test]
    fn display_includes_code() {
        assert!(
            AppError::cancelled()
                .to_string()
                .starts_with("QW-JOB-CANCELLED")
        );
    }
}
