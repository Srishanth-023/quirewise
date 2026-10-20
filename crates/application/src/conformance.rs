//! Conformance to standards: PDF/A (archive) and PDF/UA (accessibility). The ports an engine
//! implements, and what a check reports.

use std::path::Path;

use domain::tools::pdfa::PdfaLevel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::runtime::CancellationToken;

/// What a file is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The PDF/A level the file claims, or `1b` when it claims none.
    Auto,
    /// A PDF/A level.
    PdfA(PdfaLevel),
    /// PDF/UA-1, the accessibility standard.
    PdfUa1,
}

impl Profile {
    /// The name for people.
    pub fn name(self) -> String {
        match self {
            Self::Auto => "PDF/A (as claimed)".to_owned(),
            Self::PdfA(level) => level.name(),
            Self::PdfUa1 => "PDF/UA-1".to_owned(),
        }
    }
}

/// One example of a rule that was broken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Example {
    /// Where in the file, in the words of the validator.
    pub place: String,
    /// What is wrong there.
    pub message: String,
}

/// A rule of the standard that the file breaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Violation {
    /// The standard, such as `ISO 19005-2:2011`.
    pub specification: String,
    /// The clause, such as `6.2.4.3`.
    pub clause: String,
    /// The number of the test within the clause.
    pub test: u32,
    /// The rule, in the words of the standard.
    pub description: String,
    /// How many places break it.
    pub count: u32,
    /// A few of those places.
    pub examples: Vec<Example>,
}

/// The result of checking one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConformanceReport {
    /// What the file was checked against, such as `PDF/A-2b`.
    pub profile: String,
    /// True when no rule is broken.
    pub compliant: bool,
    /// Rules that held.
    pub passed_rules: u32,
    /// Rules that were broken.
    pub failed_rules: u32,
    /// Places that were checked and held.
    pub passed_checks: u32,
    /// Places that broke a rule.
    pub failed_checks: u32,
    /// The rules that were broken, in the order of the standard.
    pub violations: Vec<Violation>,
    /// The version of the program that checked.
    pub validator: String,
}

/// Checks files against PDF/A and PDF/UA. Implemented by veraPDF, an on-demand component.
pub trait PdfaValidator: Send + Sync {
    /// True when the validator is installed.
    fn is_available(&self) -> bool;

    /// Checks `path` against `profile`.
    ///
    /// Errors: `QW-COMPONENT-MISSING` when the validator is not installed, `QW-PDF-DAMAGED` when
    /// the file cannot be read, `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`.
    fn validate(
        &self,
        path: &Path,
        profile: Profile,
        cancel: &CancellationToken,
    ) -> Result<ConformanceReport, AppError>;
}

/// Makes a document fit PDF/A, as far as that can be done without changing what the pages show.
pub trait PdfaNormalizer: Send + Sync {
    /// Writes a copy of `input` (which must not be encrypted) to `dest` that follows the rules
    /// of `level` as well as it can, and returns what was done, one sentence for each thing.
    /// `now_unix` is the modification time to record, in seconds since 1970 UTC.
    fn normalize(
        &self,
        input: &Path,
        level: PdfaLevel,
        now_unix: i64,
        dest: &Path,
    ) -> Result<Vec<String>, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_have_names() {
        assert_eq!(Profile::PdfA(PdfaLevel::A2u).name(), "PDF/A-2u");
        assert_eq!(Profile::PdfUa1.name(), "PDF/UA-1");
        assert!(Profile::Auto.name().contains("claimed"));
    }
}
