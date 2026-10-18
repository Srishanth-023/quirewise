//! Jobs, per-file statuses and batch rules (Appendix A rows 59 and 60).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::ErrorCode;

/// Identifier of a job within one app session.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct JobId(pub u64);

/// Outcome of one file in a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FileStatus {
    /// Waiting to run.
    Pending,
    /// Running now.
    Running,
    /// Finished and verified.
    Done,
    /// A password is needed or was wrong.
    WrongPassword,
    /// The file is damaged.
    Damaged,
    /// A page selection fell outside the document.
    OutOfRange,
    /// Cancelled by the user.
    Cancelled,
    /// Failed for another reason; see the error code.
    Failed,
}

impl FileStatus {
    /// True for statuses that "retry failed only" should run again.
    ///
    /// Cancelled files were stopped on purpose and are not retried.
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::WrongPassword | Self::Damaged | Self::OutOfRange | Self::Failed
        )
    }

    /// True once the file will not change status without a retry.
    pub const fn is_final(self) -> bool {
        !matches!(self, Self::Pending | Self::Running)
    }

    /// Maps an error code to the per-file status shown in the batch queue.
    pub const fn from_error(code: ErrorCode) -> Self {
        match code {
            ErrorCode::PdfEncrypted | ErrorCode::PdfWrongPassword => Self::WrongPassword,
            ErrorCode::PdfDamaged | ErrorCode::InputNotPdf => Self::Damaged,
            ErrorCode::RangeOutOfBounds => Self::OutOfRange,
            ErrorCode::JobCancelled => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

/// Overall state of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum JobStatus {
    /// Accepted, not started.
    Queued,
    /// Running.
    Running,
    /// Every file is done.
    Completed,
    /// At least one file failed and none were cancelled.
    CompletedWithErrors,
    /// Stopped by the user.
    Cancelled,
}

impl JobStatus {
    /// Derives the job status from its file statuses.
    pub fn from_files(files: &[FileStatus]) -> Self {
        if files.iter().any(|s| !s.is_final()) {
            return if files.iter().all(|s| *s == FileStatus::Pending) {
                Self::Queued
            } else {
                Self::Running
            };
        }
        if files.contains(&FileStatus::Cancelled) {
            Self::Cancelled
        } else if files.iter().all(|s| *s == FileStatus::Done) {
            Self::Completed
        } else {
            Self::CompletedWithErrors
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FileStatus::*;

    #[test]
    fn retry_covers_failures_only() {
        assert!(WrongPassword.is_retryable());
        assert!(Damaged.is_retryable());
        assert!(OutOfRange.is_retryable());
        assert!(Failed.is_retryable());
        assert!(!Done.is_retryable());
        assert!(!Cancelled.is_retryable());
        assert!(!Pending.is_retryable());
    }

    #[test]
    fn error_codes_map_to_statuses() {
        assert_eq!(
            FileStatus::from_error(ErrorCode::PdfEncrypted),
            WrongPassword
        );
        assert_eq!(FileStatus::from_error(ErrorCode::PdfDamaged), Damaged);
        assert_eq!(
            FileStatus::from_error(ErrorCode::RangeOutOfBounds),
            OutOfRange
        );
        assert_eq!(FileStatus::from_error(ErrorCode::JobCancelled), Cancelled);
        assert_eq!(
            FileStatus::from_error(ErrorCode::OutputVerifyFailed),
            Failed
        );
    }

    #[test]
    fn job_status_follows_files() {
        assert_eq!(
            JobStatus::from_files(&[Pending, Pending]),
            JobStatus::Queued
        );
        assert_eq!(JobStatus::from_files(&[Done, Running]), JobStatus::Running);
        assert_eq!(JobStatus::from_files(&[Done, Done]), JobStatus::Completed);
        assert_eq!(
            JobStatus::from_files(&[Done, Damaged]),
            JobStatus::CompletedWithErrors
        );
        assert_eq!(
            JobStatus::from_files(&[Done, Cancelled]),
            JobStatus::Cancelled
        );
    }
}
