//! Stable error codes (MASTER_PROMPT.md Appendix C.3) and domain errors.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable, user-visible error code. The string form never changes once released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ErrorCode {
    /// File is not a PDF.
    #[serde(rename = "QW-INPUT-NOT-PDF")]
    InputNotPdf,
    /// Password required.
    #[serde(rename = "QW-PDF-ENCRYPTED")]
    PdfEncrypted,
    /// Password incorrect.
    #[serde(rename = "QW-PDF-WRONG-PASSWORD")]
    PdfWrongPassword,
    /// File is damaged; suggest Repair.
    #[serde(rename = "QW-PDF-DAMAGED")]
    PdfDamaged,
    /// Safety limit hit (size, objects, pages, memory).
    #[serde(rename = "QW-PDF-LIMIT-EXCEEDED")]
    PdfLimitExceeded,
    /// Page range outside the document.
    #[serde(rename = "QW-RANGE-OUT-OF-BOUNDS")]
    RangeOutOfBounds,
    /// Page range syntax error.
    #[serde(rename = "QW-RANGE-SYNTAX")]
    RangeSyntax,
    /// Option value invalid.
    #[serde(rename = "QW-OPTION-INVALID")]
    OptionInvalid,
    /// Path not allowed.
    #[serde(rename = "QW-PATH-DENIED")]
    PathDenied,
    /// Output failed verification; input untouched.
    #[serde(rename = "QW-OUTPUT-VERIFY-FAILED")]
    OutputVerifyFailed,
    /// Required component not installed; offer install.
    #[serde(rename = "QW-COMPONENT-MISSING")]
    ComponentMissing,
    /// Component hash or signature mismatch.
    #[serde(rename = "QW-COMPONENT-VERIFY-FAILED")]
    ComponentVerifyFailed,
    /// Engine exceeded its time limit.
    #[serde(rename = "QW-ENGINE-TIMEOUT")]
    EngineTimeout,
    /// Cancelled by the user.
    #[serde(rename = "QW-JOB-CANCELLED")]
    JobCancelled,
    /// Action needs network consent.
    #[serde(rename = "QW-NETWORK-CONSENT-REQUIRED")]
    NetworkConsentRequired,
    /// The network could not be reached or the server answered with an error (extension of the
    /// starter catalog).
    #[serde(rename = "QW-NETWORK-UNAVAILABLE")]
    NetworkUnavailable,
    /// Unexpected internal failure (extension of the starter catalog).
    #[serde(rename = "QW-INTERNAL")]
    Internal,
}

impl ErrorCode {
    /// The stable string form, for example `QW-RANGE-SYNTAX`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InputNotPdf => "QW-INPUT-NOT-PDF",
            Self::PdfEncrypted => "QW-PDF-ENCRYPTED",
            Self::PdfWrongPassword => "QW-PDF-WRONG-PASSWORD",
            Self::PdfDamaged => "QW-PDF-DAMAGED",
            Self::PdfLimitExceeded => "QW-PDF-LIMIT-EXCEEDED",
            Self::RangeOutOfBounds => "QW-RANGE-OUT-OF-BOUNDS",
            Self::RangeSyntax => "QW-RANGE-SYNTAX",
            Self::OptionInvalid => "QW-OPTION-INVALID",
            Self::PathDenied => "QW-PATH-DENIED",
            Self::OutputVerifyFailed => "QW-OUTPUT-VERIFY-FAILED",
            Self::ComponentMissing => "QW-COMPONENT-MISSING",
            Self::ComponentVerifyFailed => "QW-COMPONENT-VERIFY-FAILED",
            Self::EngineTimeout => "QW-ENGINE-TIMEOUT",
            Self::JobCancelled => "QW-JOB-CANCELLED",
            Self::NetworkConsentRequired => "QW-NETWORK-CONSENT-REQUIRED",
            Self::NetworkUnavailable => "QW-NETWORK-UNAVAILABLE",
            Self::Internal => "QW-INTERNAL",
        }
    }
}

impl core::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors raised by pure domain rules. Each maps to one stable [`ErrorCode`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    /// A page range could not be parsed.
    #[error("The page range \"{input}\" is not valid: {reason}. Use forms like 1,3,5-8,last.")]
    RangeSyntax {
        /// The text as entered.
        input: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A page range points outside the document.
    #[error("{detail} Valid pages are {valid}. Adjust the range and run again.")]
    RangeOutOfBounds {
        /// Which page or expression was out of bounds.
        detail: String,
        /// The valid range, for example `1-10`.
        valid: String,
    },
    /// An option value is invalid.
    #[error("The option \"{field}\" is not valid: {reason}.")]
    OptionInvalid {
        /// Option name.
        field: String,
        /// What is wrong with the value.
        reason: String,
    },
    /// A web address was given without the permission to use the internet.
    #[error(
        "Printing a web address needs the internet. Turn on \"Allow the page to load from the internet\" to agree, or choose a file instead."
    )]
    NetworkConsent,
    /// Something other than a web page needs the internet and the person has not agreed.
    #[error(
        "{what} needs the internet. Turn on \"{option}\" to agree, or change the options so nothing is sent."
    )]
    ConsentNeeded {
        /// What needs the connection.
        what: String,
        /// The label of the switch that agrees to it.
        option: String,
    },
    /// A program on this computer would be used and the person has not agreed.
    #[error("{what} needs your agreement. Turn on \"{option}\" to agree, or choose another way.")]
    LocalConsentNeeded {
        /// What would be used.
        what: String,
        /// The label of the switch that agrees to it.
        option: String,
    },
    /// A path is not allowed.
    #[error(
        "This path is not allowed: {reason}. Choose a regular file or folder on a local or mapped drive."
    )]
    PathDenied {
        /// Why the path was rejected.
        reason: String,
    },
}

impl DomainError {
    /// The stable code for this error.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::RangeSyntax { .. } => ErrorCode::RangeSyntax,
            Self::RangeOutOfBounds { .. } => ErrorCode::RangeOutOfBounds,
            Self::OptionInvalid { .. } => ErrorCode::OptionInvalid,
            Self::PathDenied { .. } => ErrorCode::PathDenied,
            Self::NetworkConsent | Self::ConsentNeeded { .. } | Self::LocalConsentNeeded { .. } => {
                ErrorCode::NetworkConsentRequired
            }
        }
    }

    /// Shorthand for an invalid option.
    pub fn option(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::OptionInvalid {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_serialize_to_their_stable_strings() {
        let json = serde_json::to_string(&ErrorCode::RangeOutOfBounds).unwrap();
        assert_eq!(json, "\"QW-RANGE-OUT-OF-BOUNDS\"");
        assert_eq!(
            ErrorCode::RangeOutOfBounds.as_str(),
            "QW-RANGE-OUT-OF-BOUNDS"
        );
    }

    #[test]
    fn every_code_round_trips_through_serde_with_matching_string() {
        let all = [
            ErrorCode::InputNotPdf,
            ErrorCode::PdfEncrypted,
            ErrorCode::PdfWrongPassword,
            ErrorCode::PdfDamaged,
            ErrorCode::PdfLimitExceeded,
            ErrorCode::RangeOutOfBounds,
            ErrorCode::RangeSyntax,
            ErrorCode::OptionInvalid,
            ErrorCode::PathDenied,
            ErrorCode::OutputVerifyFailed,
            ErrorCode::ComponentMissing,
            ErrorCode::ComponentVerifyFailed,
            ErrorCode::EngineTimeout,
            ErrorCode::JobCancelled,
            ErrorCode::NetworkConsentRequired,
            ErrorCode::NetworkUnavailable,
            ErrorCode::Internal,
        ];
        for code in all {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, code);
        }
    }

    #[test]
    fn domain_errors_map_to_codes() {
        let e = DomainError::option("dpi", "must be between 10 and 2400");
        assert_eq!(e.code(), ErrorCode::OptionInvalid);
        assert!(e.to_string().contains("dpi"));
    }
}
