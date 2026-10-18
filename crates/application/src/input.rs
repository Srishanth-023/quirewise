//! The `ValidateInput` use case: path policy, size limit and magic-byte detection, before any
//! parser sees a file.

use domain::ErrorCode;
use domain::limits::SafetyLimits;
use domain::path_rules::{UncPolicy, check_path_text};
use std::path::Path;

use crate::error::AppError;
use crate::ports::FileStore;
use crate::tool::{InputDoc, InputKind};

/// Bytes read from the start of a file for detection.
pub const SNIFF_BYTES: usize = 1024;

/// Detects the kind of a file from its first bytes. Returns `None` for anything unknown.
///
/// PDFs may have junk before the header, so `%PDF-` is accepted within the first 1,024 bytes.
pub fn sniff(head: &[u8]) -> Option<InputKind> {
    if head.windows(5).any(|w| w == b"%PDF-") {
        return Some(InputKind::Pdf);
    }
    let starts = |magic: &[u8]| head.starts_with(magic);
    let is_image = starts(&[0xFF, 0xD8, 0xFF])
        || starts(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
        || starts(b"GIF87a")
        || starts(b"GIF89a")
        || starts(b"BM")
        || starts(&[b'I', b'I', 0x2A, 0x00])
        || starts(&[b'M', b'M', 0x00, 0x2A])
        || (head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP")
        || (head.len() >= 12
            && &head[4..8] == b"ftyp"
            && [&b"heic"[..], b"heix", b"hevc", b"mif1", b"msf1", b"heif"]
                .iter()
                .any(|brand| &head[8..12] == *brand));
    is_image.then_some(InputKind::Image)
}

/// Validates one input path and returns it as an [`InputDoc`] with a canonical path.
///
/// Errors: `QW-PATH-DENIED` (policy, missing, too large) and `QW-INPUT-NOT-PDF` (unknown kind
/// or a kind the tool does not accept).
pub fn validate_input(
    files: &dyn FileStore,
    path: &Path,
    accepts: &[InputKind],
    limits: &SafetyLimits,
) -> Result<(InputDoc, InputKind), AppError> {
    check_path_text(&path.to_string_lossy(), UncPolicy::UserPicked)?;
    let canonical = files.canonicalize(path)?;
    // The canonical form can reveal a device path behind a link, so check it again.
    check_path_text(&canonical.to_string_lossy(), UncPolicy::UserPicked)?;
    let size = files.len(&canonical)?;
    if size > limits.max_input_size.get() {
        return Err(AppError::new(
            ErrorCode::PdfLimitExceeded,
            "This file is larger than the safety limit. Split it with another tool first or raise the limit in Settings.",
        ));
    }
    let head = files.read_head(&canonical, SNIFF_BYTES)?;
    let kind = sniff(&head).ok_or_else(not_supported)?;
    if !accepts.contains(&kind) {
        return Err(not_supported());
    }
    Ok((InputDoc::new(canonical), kind))
}

fn not_supported() -> AppError {
    AppError::new(
        ErrorCode::InputNotPdf,
        "This file is not a type this tool accepts. Choose a PDF (or an image for image tools).",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryFiles;

    #[test]
    fn sniffs_pdf_and_images() {
        assert_eq!(sniff(b"%PDF-1.7\n"), Some(InputKind::Pdf));
        assert_eq!(sniff(b"\0\0junk%PDF-1.4"), Some(InputKind::Pdf));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(InputKind::Image));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n"), Some(InputKind::Image));
        assert_eq!(sniff(b"GIF89a"), Some(InputKind::Image));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(InputKind::Image));
        assert_eq!(sniff(b"\0\0\0\x18ftypheic"), Some(InputKind::Image));
        assert_eq!(sniff(b"II*\0"), Some(InputKind::Image));
        assert_eq!(sniff(b"PK\x03\x04"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn accepts_a_pdf_and_canonicalizes() {
        let files = MemoryFiles::default();
        files.put("C:\\in\\a.pdf", b"%PDF-1.7 body");
        let (doc, kind) = validate_input(
            &files,
            Path::new("C:\\in\\a.pdf"),
            &[InputKind::Pdf],
            &SafetyLimits::default(),
        )
        .unwrap();
        assert_eq!(kind, InputKind::Pdf);
        assert_eq!(doc.path, Path::new("C:\\in\\a.pdf"));
    }

    #[test]
    fn rejects_wrong_kind_missing_oversized_and_device_paths() {
        let files = MemoryFiles::default();
        files.put("C:\\in\\a.zip", b"PK\x03\x04");
        files.put("C:\\in\\big.pdf", b"%PDF-1.7");
        let limits = SafetyLimits::default();
        let pdf = [InputKind::Pdf];

        let wrong = validate_input(&files, Path::new("C:\\in\\a.zip"), &pdf, &limits).unwrap_err();
        assert_eq!(wrong.code, ErrorCode::InputNotPdf);

        let missing =
            validate_input(&files, Path::new("C:\\in\\none.pdf"), &pdf, &limits).unwrap_err();
        assert_eq!(missing.code, ErrorCode::PathDenied);

        let device = validate_input(&files, Path::new("\\\\.\\C:"), &pdf, &limits).unwrap_err();
        assert_eq!(device.code, ErrorCode::PathDenied);

        let tiny = SafetyLimits {
            max_input_size: domain::Bytes::new(4),
            ..SafetyLimits::default()
        };
        let big = validate_input(&files, Path::new("C:\\in\\big.pdf"), &pdf, &tiny).unwrap_err();
        assert_eq!(big.code, ErrorCode::PdfLimitExceeded);

        let image_only = validate_input(
            &files,
            Path::new("C:\\in\\big.pdf"),
            &[InputKind::Image],
            &limits,
        )
        .unwrap_err();
        assert_eq!(image_only.code, ErrorCode::InputNotPdf);
    }
}
