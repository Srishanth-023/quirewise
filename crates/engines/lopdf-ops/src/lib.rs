//! lopdf adapter: raw structure facts (encryption, JavaScript, PDF/A claim, size breakdown).
//!
//! lopdf is pure Rust and parses the file without rendering. It reads untrusted bytes, so it only
//! runs inside the worker, with a cap on how far any stream may expand.
#![forbid(unsafe_code)]

mod annotations;
mod boxes;
mod edit;
mod encryption;
mod metadata;
mod outline;
mod sanitize;
mod scan;
mod toc;

pub use toc::LopdfToc;

use application::AppError;
use application::Password;
use application::inspect::StructureReport;
use application::ports::{PdfEdit, PdfEditor, StructureFacts};
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

/// [`PdfEditor`] over lopdf. It rewrites the whole file, so it is used on finished, unencrypted
/// outputs, never on user files directly.
#[derive(Debug, Clone, Copy, Default)]
pub struct LopdfEditor;

impl PdfEditor for LopdfEditor {
    fn apply(&self, source: &Path, edit: &PdfEdit, dest: &Path) -> Result<(), AppError> {
        let mut doc = LopdfStructure::load(source, None)?;
        edit::apply(&mut doc, edit)?;
        doc.save(dest)
            .map_err(|e| AppError::from_io("write the edited document", &e))?;
        Ok(())
    }
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

    // ---- editor ----

    use application::ports::{OutlineEntry, PdfEdit, PdfEditor};
    use domain::PageIndex;
    use domain::tools::common::FractionBox;

    fn edit_file(dir: &tempfile::TempDir, pdf: &[u8], edit: &PdfEdit) -> Document {
        let source = write(dir, "in.pdf", pdf);
        let dest = dir.path().join("out.pdf");
        LopdfEditor.apply(&source, edit, &dest).unwrap();
        Document::load(&dest).unwrap()
    }

    fn page_ids(doc: &Document) -> Vec<lopdf::ObjectId> {
        doc.get_pages().values().copied().collect()
    }

    #[test]
    fn crop_fractions_set_the_crop_box_to_the_chosen_half() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = test_support::pdfs::pdf_with_geometry(&[(600, 800, 0), (600, 800, 0)]);
        let left = FractionBox {
            left: 0.0,
            bottom: 0.0,
            right: 0.5,
            top: 1.0,
        };
        let right = FractionBox {
            left: 0.5,
            bottom: 0.0,
            right: 1.0,
            top: 1.0,
        };
        let doc = edit_file(
            &dir,
            &pdf,
            &PdfEdit::CropFractions(vec![(PageIndex::new(0), left), (PageIndex::new(1), right)]),
        );
        let ids = page_ids(&doc);
        assert_eq!(
            crate::boxes::visible_box(&doc, ids[0]),
            [0.0, 0.0, 300.0, 800.0]
        );
        assert_eq!(
            crate::boxes::visible_box(&doc, ids[1]),
            [300.0, 0.0, 600.0, 800.0]
        );
    }

    #[test]
    fn crop_fractions_respect_page_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = test_support::pdfs::pdf_with_geometry(&[(600, 800, 90)]);
        let left = FractionBox {
            left: 0.0,
            bottom: 0.0,
            right: 0.5,
            top: 1.0,
        };
        let doc = edit_file(
            &dir,
            &pdf,
            &PdfEdit::CropFractions(vec![(PageIndex::new(0), left)]),
        );
        // Turned a quarter clockwise, the displayed left half is the bottom of the stored page.
        assert_eq!(
            crate::boxes::visible_box(&doc, page_ids(&doc)[0]),
            [0.0, 0.0, 600.0, 400.0]
        );
    }

    #[test]
    fn scaling_gives_every_page_the_target_size_and_wraps_its_content() {
        let dir = tempfile::tempdir().unwrap();
        let pdf =
            test_support::pdfs::pdf_with_geometry(&[(300, 400, 0), (612, 792, 0), (400, 300, 90)]);
        let doc = edit_file(
            &dir,
            &pdf,
            &PdfEdit::ScalePagesTo {
                width_pt: 612.0,
                height_pt: 792.0,
            },
        );
        for id in page_ids(&doc) {
            let [x0, y0, x1, y1] = crate::boxes::visible_box(&doc, id);
            let (w, h) = (x1 - x0, y1 - y0);
            let turned = crate::boxes::rotation(&doc, id) % 180 == 90;
            let (dw, dh) = if turned { (h, w) } else { (w, h) };
            assert_eq!((dw.round(), dh.round()), (612.0, 792.0));
        }
        let first = page_ids(&doc)[0];
        let contents = doc
            .get_dictionary(first)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_array()
            .unwrap()
            .len();
        assert_eq!(
            contents, 3,
            "a prefix stream, the original content, and a suffix stream"
        );
        // The 300x400 page is scaled by 1.98 (limited by height), centered horizontally.
        let prefix = doc
            .get_object(
                doc.get_dictionary(first)
                    .unwrap()
                    .get(b"Contents")
                    .unwrap()
                    .as_array()
                    .unwrap()[0]
                    .as_reference()
                    .unwrap(),
            )
            .unwrap();
        let text = String::from_utf8_lossy(&prefix.as_stream().unwrap().content).into_owned();
        assert!(text.contains("1.98000 0 0 1.98000"), "got {text:?}");
    }

    #[test]
    fn outlines_nest_and_point_at_pages() {
        let dir = tempfile::tempdir().unwrap();
        let entry = |title: &str, page: u32, level: u32| OutlineEntry {
            title: title.to_owned(),
            page: PageIndex::new(page),
            level,
        };
        let doc = edit_file(
            &dir,
            &test_support::pdfs::pdf_with_texts(&["a", "b", "c", "d"]),
            &PdfEdit::SetOutline(vec![
                entry("Part one", 0, 0),
                entry("Chapter A", 1, 1),
                entry("Part two", 2, 0),
                entry("Beyond the end", 99, 0),
            ]),
        );
        let catalog = doc.catalog().unwrap();
        let root = doc
            .get_dictionary(catalog.get(b"Outlines").unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(
            root.get(b"Count").unwrap().as_i64().unwrap(),
            3,
            "out-of-range bookmarks are dropped"
        );
        let first = doc
            .get_dictionary(root.get(b"First").unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(first.get(b"Title").unwrap().as_str().unwrap(), b"Part one");
        assert_eq!(
            first.get(b"Count").unwrap().as_i64().unwrap(),
            1,
            "one nested child"
        );
        let child = doc
            .get_dictionary(first.get(b"First").unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(child.get(b"Title").unwrap().as_str().unwrap(), b"Chapter A");
        let second = doc
            .get_dictionary(first.get(b"Next").unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(second.get(b"Title").unwrap().as_str().unwrap(), b"Part two");
        let dest = second.get(b"Dest").unwrap().as_array().unwrap();
        assert_eq!(dest[0].as_reference().unwrap(), page_ids(&doc)[2]);
    }

    #[test]
    fn an_empty_outline_removes_existing_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = test_support::pdfs::pdf_with_outline(2, &[("Only", 0, 1)]);
        let doc = edit_file(&dir, &pdf, &PdfEdit::SetOutline(Vec::new()));
        assert!(doc.catalog().unwrap().get(b"Outlines").is_err());
    }
}
