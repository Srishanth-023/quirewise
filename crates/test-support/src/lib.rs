//! Test helpers shared by engine and integration tests. Never a normal dependency.
#![forbid(unsafe_code)]
// Helpers panic on a broken test environment by design.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

/// Repository root, found from this crate's manifest folder.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Architecture folder name used under `vendor/<name>/`.
pub const fn arch_dir() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Folder holding the fetched PDFium library (`scripts/fetch-pdfium.ps1`).
pub fn pdfium_dir() -> PathBuf {
    repo_root()
        .join("vendor")
        .join("pdfium")
        .join(arch_dir())
        .join("bin")
}

/// Path of the fetched `qpdf.exe` (`scripts/fetch-qpdf.ps1`), or `None` when not fetched.
pub fn qpdf_exe() -> Option<PathBuf> {
    let base = repo_root().join("vendor").join("qpdf").join(arch_dir());
    std::fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path().join("bin").join("qpdf.exe"))
        .find(|p| p.is_file())
}

/// Builds a valid, uncompressed PDF with `pages` Letter-size pages. Page `n` shows the text
/// `Page n of N`, so text extraction and rendering have something real to find.
pub fn minimal_pdf(pages: usize) -> Vec<u8> {
    minimal_pdf_with_catalog(pages, "")
}

/// Like [`minimal_pdf`], with extra entries (for example an `/OpenAction`) in the catalog.
pub fn minimal_pdf_with_catalog(pages: usize, catalog_extra: &str) -> Vec<u8> {
    let pages = pages.max(1);
    // Object layout: 1 catalog, 2 page tree, 3 font, then (page, content) pairs.
    let mut objects: Vec<String> = Vec::new();
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
    objects.push(format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>"));
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned());
    for i in 0..pages {
        let content = format!("BT /F1 24 Tf 72 700 Td (Page {} of {pages}) Tj ET", i + 1);
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + i * 2
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }

    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Encrypts `source` into `dest` with AES-256 using the fetched qpdf. The user password is
/// `user_password`; the owner password is `owner`.
///
/// # Panics
/// Panics when qpdf is missing or fails, which means the test environment is incomplete.
pub fn encrypt_pdf(source: &Path, dest: &Path, user_password: &str) {
    let exe = qpdf_exe().expect("run scripts/fetch-qpdf.ps1 first");
    let status = std::process::Command::new(exe)
        .args(["--encrypt", user_password, "owner", "256", "--"])
        .arg(source)
        .arg(dest)
        .status()
        .expect("qpdf starts");
    assert!(status.success(), "qpdf could not encrypt the fixture");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_pdf_has_header_pages_and_trailer() {
        let pdf = minimal_pdf(3);
        let text = String::from_utf8_lossy(&pdf);
        assert!(pdf.starts_with(b"%PDF-1.7"));
        assert!(text.contains("/Count 3"));
        assert!(text.contains("Page 2 of 3"));
        assert!(text.trim_end().ends_with("%%EOF"));
    }

    #[test]
    fn repo_root_contains_the_workspace_manifest() {
        assert!(repo_root().join("Cargo.toml").is_file());
    }
}
