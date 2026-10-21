//! Finds the bundled native binaries (PDFium and qpdf).
//!
//! Installed layout: `<exe folder>\native\pdfium\pdfium.dll` and `<exe folder>\native\qpdf\qpdf.exe`.
//! Development layout: `vendor\pdfium\<arch>\bin` and `vendor\qpdf\<arch>\<version>\bin`, fetched
//! by `scripts/fetch/fetch-pdfium.ps1` and `scripts/fetch/fetch-qpdf.ps1`.

use application::AppError;
use domain::ErrorCode;
use std::fs;
use std::path::{Path, PathBuf};

/// Architecture folder name used by the fetch scripts.
pub const fn arch_dir() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Environment variable that points at an explicit native folder (installed layout).
pub const NATIVE_DIR_VAR: &str = "QW_NATIVE_DIR";

/// Locations of the native binaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePaths {
    /// Folder containing `pdfium.dll`.
    pub pdfium_dir: PathBuf,
    /// Full path of `qpdf.exe`.
    pub qpdf_exe: PathBuf,
}

fn find_pdfium(root: &Path, arch: &str) -> Option<PathBuf> {
    [
        root.join("native").join("pdfium"),
        root.join("vendor").join("pdfium").join(arch).join("bin"),
    ]
    .into_iter()
    .find(|dir| dir.join("pdfium.dll").is_file())
}

fn find_qpdf(root: &Path, arch: &str) -> Option<PathBuf> {
    let installed = root.join("native").join("qpdf").join("qpdf.exe");
    if installed.is_file() {
        return Some(installed);
    }
    let versions = fs::read_dir(root.join("vendor").join("qpdf").join(arch)).ok()?;
    versions
        .flatten()
        .map(|v| v.path().join("bin").join("qpdf.exe"))
        .find(|p| p.is_file())
}

impl NativePaths {
    /// Looks for both binaries: first in `QW_NATIVE_DIR`, then in the exe folder and up to six
    /// parent folders (so development builds find `vendor\`).
    pub fn locate(
        env: &dyn Fn(&str) -> Option<String>,
        exe_dir: &Path,
        arch: &str,
    ) -> Result<Self, AppError> {
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(dir) = env(NATIVE_DIR_VAR).filter(|d| !d.is_empty()) {
            roots.push(PathBuf::from(dir));
        }
        roots.extend(exe_dir.ancestors().take(7).map(Path::to_path_buf));

        let pdfium_dir = roots.iter().find_map(|r| find_pdfium(r, arch));
        let qpdf_exe = roots.iter().find_map(|r| find_qpdf(r, arch));
        match (pdfium_dir, qpdf_exe) {
            (Some(pdfium_dir), Some(qpdf_exe)) => Ok(Self {
                pdfium_dir,
                qpdf_exe,
            }),
            _ => Err(AppError::new(
                ErrorCode::ComponentMissing,
                "The PDF engine files (pdfium.dll and qpdf.exe) were not found. Reinstall Quirewise, or run the fetch scripts when working from source.",
            )),
        }
    }

    /// Locates from the real environment and the running executable.
    pub fn from_environment() -> Result<Self, AppError> {
        let exe = std::env::current_exe().map_err(|e| AppError::from_io("locate the app", &e))?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| AppError::internal("locating the app folder"))?;
        Self::locate(&|name| std::env::var(name).ok(), exe_dir, arch_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn finds_the_installed_layout_next_to_the_exe() {
        let root = tempdir().unwrap();
        touch(&root.path().join("native/pdfium/pdfium.dll"));
        touch(&root.path().join("native/qpdf/qpdf.exe"));
        let p = NativePaths::locate(&|_| None, root.path(), "x64").unwrap();
        assert_eq!(p.pdfium_dir, root.path().join("native/pdfium"));
        assert_eq!(p.qpdf_exe, root.path().join("native/qpdf/qpdf.exe"));
    }

    #[test]
    fn finds_the_vendor_layout_from_a_nested_build_folder() {
        let root = tempdir().unwrap();
        touch(&root.path().join("vendor/pdfium/arm64/bin/pdfium.dll"));
        touch(
            &root
                .path()
                .join("vendor/qpdf/arm64/qpdf-12.4.2-msvc-arm64/bin/qpdf.exe"),
        );
        let exe_dir = root.path().join("target/debug/deps");
        fs::create_dir_all(&exe_dir).unwrap();
        let p = NativePaths::locate(&|_| None, &exe_dir, "arm64").unwrap();
        assert!(
            p.pdfium_dir.ends_with("vendor/pdfium/arm64/bin")
                || p.pdfium_dir.ends_with(r"vendor\pdfium\arm64\bin")
        );
        assert!(p.qpdf_exe.ends_with("qpdf.exe"));
        assert!(
            NativePaths::locate(&|_| None, &exe_dir, "x64").is_err(),
            "wrong arch is not used"
        );
    }

    #[test]
    fn the_environment_variable_wins() {
        let custom = tempdir().unwrap();
        touch(&custom.path().join("native/pdfium/pdfium.dll"));
        touch(&custom.path().join("native/qpdf/qpdf.exe"));
        let elsewhere = tempdir().unwrap();
        touch(&elsewhere.path().join("native/pdfium/pdfium.dll"));
        touch(&elsewhere.path().join("native/qpdf/qpdf.exe"));
        let custom_dir = custom.path().to_string_lossy().into_owned();
        let p = NativePaths::locate(
            &|n| (n == NATIVE_DIR_VAR).then(|| custom_dir.clone()),
            elsewhere.path(),
            "x64",
        )
        .unwrap();
        assert!(p.pdfium_dir.starts_with(custom.path()));
    }

    #[test]
    fn missing_binaries_report_a_missing_component() {
        let root = tempdir().unwrap();
        let err = NativePaths::locate(&|_| None, root.path(), "x64").unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }
}
