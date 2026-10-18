//! lopdf adapter: raw structure facts (encryption, JavaScript, PDF/A claim, size breakdown).
//!
//! lopdf is pure Rust and parses the file without rendering. It reads untrusted bytes, so it only
//! runs inside the worker, with a cap on how far any stream may expand.
#![forbid(unsafe_code)]

mod encryption;
mod scan;

use application::AppError;
use application::Password;
use application::inspect::StructureReport;
use application::ports::StructureFacts;
use domain::ErrorCode;
use lopdf::{Document, LoadOptions};
use std::fs;
use std::path::Path;

/// Largest size any one stream may expand to while loading (decompression bomb guard).
pub const MAX_STREAM_EXPANSION: usize = 256 * 1024 * 1024;

/// [`StructureFacts`] implementation over lopdf.
#[derive(Debug, Clone, Copy, Default)]
pub struct LopdfStructure;

fn damaged() -> AppError {
    AppError::new(
        ErrorCode::PdfDamaged,
        "This file is damaged or uses an unsupported format. Try the Repair tool.",
    )
}

impl LopdfStructure {
    fn load(path: &Path, password: Option<&Password>) -> Result<Document, AppError> {
        let options = LoadOptions {
            password: password.map(|p| p.expose().to_owned()),
            max_decompressed_size: Some(MAX_STREAM_EXPANSION),
            ..LoadOptions::default()
        };
        Document::load_with_options(path, options).map_err(|e| match e {
            lopdf::Error::InvalidPassword => AppError::new(
                ErrorCode::PdfWrongPassword,
                "The password is not correct. Check it and try again.",
            ),
            _ => damaged(),
        })
    }
}

impl StructureFacts for LopdfStructure {
    fn structure(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<StructureReport, AppError> {
        let file_size = fs::metadata(path)
            .map_err(|e| AppError::from_io("read the file", &e))?
            .len();
        // The first load never uses the password: the Encrypt dictionary is stored in the clear
        // and lopdf drops it from the document once it decrypts.
        let first = Self::load(path, None)?;
        let Some((info, permissions)) = encryption::describe(&first) else {
            return Ok(scan::report(&first, file_size));
        };
        let Some(password) = password else {
            return Ok(StructureReport {
                encryption: Some(info),
                permissions,
                ..StructureReport::default()
            });
        };
        let unlocked = Self::load(path, Some(password))?;
        let mut report = scan::report(&unlocked, file_size);
        report.encryption = Some(info);
        report.permissions = permissions;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use test_support::{minimal_pdf, minimal_pdf_with_catalog, qpdf_exe};

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn encrypt(dir: &tempfile::TempDir, source: &Path, key_bits: &str) -> PathBuf {
        let out = dir.path().join(format!("locked-{key_bits}.pdf"));
        let mut args = vec!["--encrypt", "open", "owner", key_bits];
        if key_bits == "128" {
            // qpdf writes RC4 for 128-bit keys unless AES is requested; 256-bit is always AES.
            args.push("--use-aes=y");
        }
        args.extend(["--print=low", "--extract=n", "--"]);
        let status = std::process::Command::new(qpdf_exe().unwrap())
            .args(args)
            .arg(source)
            .arg(&out)
            .status()
            .unwrap();
        assert!(status.success());
        out
    }

    #[test]
    fn plain_pdf_has_no_encryption_and_sizes_add_up() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = minimal_pdf(3);
        let path = write(&dir, "a.pdf", &pdf);
        let r = LopdfStructure.structure(&path, None).unwrap();
        assert!(r.encryption.is_none());
        assert!(!r.has_javascript);
        assert_eq!(r.pdfa_claim, None);
        assert_eq!(r.embedded_files, 0);
        let s = r.size_breakdown;
        assert!(s.content > 0, "page content streams are counted");
        assert_eq!(s.images + s.fonts + s.content + s.other, pdf.len() as u64);
    }

    #[test]
    fn javascript_in_an_open_action_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = minimal_pdf_with_catalog(1, "/OpenAction << /S /JavaScript /JS (app.alert) >>");
        let r = LopdfStructure
            .structure(&write(&dir, "js.pdf", &pdf), None)
            .unwrap();
        assert!(r.has_javascript);
    }

    #[test]
    fn pdfa_claim_comes_from_the_xmp_metadata() {
        let xmp = "<x:xmpmeta><rdf:RDF><rdf:Description><pdfaid:part>2</pdfaid:part><pdfaid:conformance>B</pdfaid:conformance></rdf:Description></rdf:RDF></x:xmpmeta>";
        // The metadata stream is object 100, referenced from the catalog.
        let mut pdf = minimal_pdf_with_catalog(1, "/Metadata 100 0 R");
        let marker = b"trailer";
        let at = pdf.windows(marker.len()).position(|w| w == marker).unwrap();
        let obj = format!(
            "100 0 obj\n<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{xmp}\nendstream\nendobj\n",
            xmp.len()
        );
        pdf.splice(at..at, obj.into_bytes());
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "pdfa.pdf", &pdf);
        // The hand-edited file has a stale xref table; lopdf rebuilds it when it can.
        let r = LopdfStructure.structure(&path, None).unwrap();
        assert_eq!(r.pdfa_claim.as_deref(), Some("PDF/A-2b"));
    }

    #[test]
    fn encrypted_files_report_cipher_and_permissions_without_a_password() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write(&dir, "a.pdf", &minimal_pdf(2));
        for (bits, method) in [("256", "AES-256"), ("128", "AES-128")] {
            let locked = encrypt(&dir, &plain, bits);
            let r = LopdfStructure.structure(&locked, None).unwrap();
            let info = r.encryption.expect("encryption info");
            assert_eq!(info.method, method, "key size {bits}");
            assert_eq!(info.key_bits, Some(bits.parse().unwrap()));
            let perms = r.permissions.expect("permissions");
            assert!(perms.print, "low-quality print allowed");
            assert!(!perms.copy, "extraction disabled");
        }
    }

    #[test]
    fn encrypted_files_open_with_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write(&dir, "a.pdf", &minimal_pdf(2));
        let locked = encrypt(&dir, &plain, "256");
        let r = LopdfStructure
            .structure(&locked, Some(&Password::new("open")))
            .unwrap();
        assert_eq!(r.encryption.map(|e| e.method), Some("AES-256".to_owned()));
        assert!(
            r.size_breakdown.content > 0,
            "decrypted content is measured"
        );
    }

    #[test]
    fn damaged_files_report_damage() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "j.pdf", b"nothing like a pdf");
        assert_eq!(
            LopdfStructure.structure(&path, None).unwrap_err().code,
            ErrorCode::PdfDamaged
        );
        let bomb_guard = write(&dir, "t.pdf", b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog");
        assert!(LopdfStructure.structure(&bomb_guard, None).is_err());
    }
}
