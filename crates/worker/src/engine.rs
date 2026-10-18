//! Engine wiring for the worker. Real adapters (PDFium, qpdf, ...) are plugged in here as the
//! phases add them; until then the PDF reader reports that no engine is available.

use application::AppError;
use application::Password;
use application::ports::{DocumentInfo, PdfReader};
use domain::ErrorCode;
use std::path::Path;

/// Placeholder reader used until the PDFium adapter lands in Phase 2.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoEngine;

impl PdfReader for NoEngine {
    fn open_info(
        &self,
        _path: &Path,
        _password: Option<&Password>,
    ) -> Result<DocumentInfo, AppError> {
        Err(AppError::new(
            ErrorCode::ComponentMissing,
            "The PDF engine is not part of this build yet. Update Quirewise to a version that includes it.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_missing_engine_with_a_catalog_code() {
        let err = NoEngine.open_info(Path::new("a.pdf"), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }
}
