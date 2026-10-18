//! Cancellation, progress and secret handling shared by use cases and tools.

use core::fmt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeroize::Zeroizing;

use crate::error::AppError;

/// Cooperative cancellation flag. Clones share the same flag.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Creates a token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// True once cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Returns `QW-JOB-CANCELLED` when cancelled. Call this once per page.
    pub fn check(&self) -> Result<(), AppError> {
        if self.is_cancelled() {
            Err(AppError::cancelled())
        } else {
            Ok(())
        }
    }
}

/// Determinate progress, for example "Page 14 of 120".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressEvent {
    /// Units finished so far (pages or files).
    pub done: u64,
    /// Total units, always known up front so the bar is determinate.
    pub total: u64,
}

/// Receives progress from running tools.
pub trait ProgressSink: Send + Sync {
    /// Reports progress. Must be cheap and never block.
    fn report(&self, event: ProgressEvent);
}

/// A sink that drops events, for tests and the headless CLI.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn report(&self, _event: ProgressEvent) {}
}

/// A document password held in memory only: cleared on drop, redacted in `Debug`, never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(Zeroizing<String>);

impl Password {
    /// Wraps a password.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// The secret text. Pass it straight to the engine; do not store or log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(<redacted>)")
    }
}

// The worker needs the password over its private stdio pipe, so it serializes as a plain string.
impl Serialize for Password {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Password {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_shared_between_clones() {
        let a = CancellationToken::new();
        let b = a.clone();
        assert!(b.check().is_ok());
        a.cancel();
        assert!(b.is_cancelled());
        assert_eq!(b.check().unwrap_err().code, domain::ErrorCode::JobCancelled);
    }

    #[test]
    fn passwords_are_redacted_in_debug_output() {
        let p = Password::new("hunter2");
        assert!(!format!("{p:?}").contains("hunter2"));
        assert_eq!(p.expose(), "hunter2");
    }

    #[test]
    fn passwords_round_trip_over_ipc() {
        let json = serde_json::to_string(&Password::new("pw")).unwrap();
        let back: Password = serde_json::from_str(&json).unwrap();
        assert_eq!(back.expose(), "pw");
    }
}
