//! Maps PDFium and `pdfium-render` failures to the stable error catalog. Raw library text never
//! reaches the user.

use application::AppError;
use domain::ErrorCode;
use pdfium_render::prelude::{PdfiumError, PdfiumInternalError};

/// Converts a PDFium error. `had_password` tells apart "needs a password" from "wrong password",
/// which PDFium reports with the same code.
pub fn map_error(error: &PdfiumError, had_password: bool) -> AppError {
    match error {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            if had_password {
                AppError::new(
                    ErrorCode::PdfWrongPassword,
                    "The password is not correct. Check it and try again.",
                )
            } else {
                AppError::new(
                    ErrorCode::PdfEncrypted,
                    "This file is password protected. Enter the password to open it.",
                )
            }
        }
        PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::FormatError | PdfiumInternalError::SecurityError,
        ) => AppError::new(
            ErrorCode::PdfDamaged,
            "This file is damaged or uses an unsupported format. Try the Repair tool.",
        ),
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FileError) => AppError::new(
            ErrorCode::PathDenied,
            "The file could not be read. Check that it exists and is not open in another program.",
        ),
        PdfiumError::IoError(io) => AppError::from_io("open the file", io),
        PdfiumError::PageIndexOutOfBounds => domain::DomainError::RangeOutOfBounds {
            detail: "That page is outside this document.".to_owned(),
            valid: "the page count".to_owned(),
        }
        .into(),
        _ => AppError::internal("reading the PDF"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_errors_depend_on_whether_one_was_given() {
        let e = PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError);
        assert_eq!(map_error(&e, false).code, ErrorCode::PdfEncrypted);
        assert_eq!(map_error(&e, true).code, ErrorCode::PdfWrongPassword);
    }

    #[test]
    fn format_errors_suggest_repair() {
        let e = PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError);
        let mapped = map_error(&e, false);
        assert_eq!(mapped.code, ErrorCode::PdfDamaged);
        assert!(mapped.message.contains("Repair"));
    }

    #[test]
    fn unknown_failures_are_internal_without_library_text() {
        let e = PdfiumError::UnknownBitmapFormat;
        let mapped = map_error(&e, false);
        assert_eq!(mapped.code, ErrorCode::Internal);
        assert!(!mapped.message.contains("Bitmap"));
    }
}
