//! Batch semantics (Appendix A row 60): units per input mode, per-file status, retry failed only.

use domain::ToolId;
use domain::job::{FileStatus, JobStatus};
use std::path::PathBuf;

use crate::error::AppError;
use crate::tool::{InputDoc, InputMode, OutputArtifact};

/// Splits a batch into units: one per file, or one unit holding every file.
pub fn split_units(mode: InputMode, inputs: &[InputDoc]) -> Vec<Vec<InputDoc>> {
    match mode {
        InputMode::PerFile => inputs.iter().map(|i| vec![i.clone()]).collect(),
        InputMode::AllTogether if inputs.is_empty() => Vec::new(),
        InputMode::AllTogether => vec![inputs.to_vec()],
    }
}

/// Result of one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    /// Input paths of the unit.
    pub inputs: Vec<PathBuf>,
    /// Status shown in the batch queue.
    pub status: FileStatus,
    /// The error, when the unit did not finish.
    pub error: Option<AppError>,
    /// Outputs, when the unit finished.
    pub outputs: Vec<OutputArtifact>,
}

impl FileReport {
    /// A unit that has not run yet.
    pub fn pending(inputs: Vec<PathBuf>) -> Self {
        Self {
            inputs,
            status: FileStatus::Pending,
            error: None,
            outputs: Vec::new(),
        }
    }

    /// Records the result of running the unit.
    pub fn finish(&mut self, result: Result<Vec<OutputArtifact>, AppError>) {
        match result {
            Ok(outputs) => {
                self.status = FileStatus::Done;
                self.error = None;
                self.outputs = outputs;
            }
            Err(error) => {
                self.status = FileStatus::from_error(error.code);
                self.error = Some(error);
                self.outputs.clear();
            }
        }
    }
}

/// Per-file results of a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport {
    /// Tool that ran.
    pub tool: ToolId,
    /// One entry per unit, in input order.
    pub files: Vec<FileReport>,
}

impl BatchReport {
    /// Overall job status derived from the files.
    pub fn status(&self) -> JobStatus {
        let statuses: Vec<FileStatus> = self.files.iter().map(|f| f.status).collect();
        JobStatus::from_files(&statuses)
    }

    /// Indices of units that "retry failed only" should run again.
    pub fn retryable(&self) -> Vec<usize> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.status.is_retryable())
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ErrorCode;

    fn docs(n: usize) -> Vec<InputDoc> {
        (0..n)
            .map(|i| InputDoc::new(format!("C:\\in\\{i}.pdf")))
            .collect()
    }

    #[test]
    fn units_follow_the_input_mode() {
        assert_eq!(split_units(InputMode::PerFile, &docs(3)).len(), 3);
        let together = split_units(InputMode::AllTogether, &docs(3));
        assert_eq!(together.len(), 1);
        assert_eq!(together[0].len(), 3);
        assert!(split_units(InputMode::AllTogether, &[]).is_empty());
    }

    #[test]
    fn retry_picks_failed_units_only() {
        let mut report = BatchReport {
            tool: ToolId::Rotate,
            files: (0..4)
                .map(|i| FileReport::pending(vec![PathBuf::from(format!("{i}.pdf"))]))
                .collect(),
        };
        report.files[0].finish(Ok(Vec::new()));
        report.files[1].finish(Err(AppError::new(ErrorCode::PdfEncrypted, "locked")));
        report.files[2].finish(Err(AppError::cancelled()));
        report.files[3].finish(Err(AppError::new(ErrorCode::PdfDamaged, "bad")));
        assert_eq!(report.retryable(), vec![1, 3]);
        assert_eq!(report.files[1].status, FileStatus::WrongPassword);
        assert_eq!(report.files[3].status, FileStatus::Damaged);
        assert_eq!(report.status(), JobStatus::Cancelled);
    }

    #[test]
    fn retry_success_clears_the_error() {
        let mut f = FileReport::pending(vec![PathBuf::from("a.pdf")]);
        f.finish(Err(AppError::new(ErrorCode::PdfDamaged, "bad")));
        f.finish(Ok(Vec::new()));
        assert_eq!(f.status, FileStatus::Done);
        assert!(f.error.is_none());
    }
}
