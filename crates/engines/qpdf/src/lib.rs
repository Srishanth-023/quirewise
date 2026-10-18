//! qpdf adapter: structural checks and page copying through the `qpdf` command line sidecar.
//!
//! qpdf runs as a separate process and is never linked in. Passwords are passed on standard
//! input (`--password-file=-`), never as arguments or temp files, so they cannot show up in a
//! process list or on disk.
#![forbid(unsafe_code)]

mod process;

use application::AppError;
use application::Password;
use application::ports::PdfWriter;
use domain::{ErrorCode, PageIndex};
use process::{Output, run};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Default time limit for one qpdf run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// How a qpdf run ended when it did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    /// No errors and no warnings.
    Clean,
    /// qpdf repaired or tolerated something (exit code 3).
    Warnings,
}

/// Handle to the qpdf sidecar executable.
#[derive(Debug, Clone)]
pub struct QpdfCli {
    exe: PathBuf,
    timeout: Duration,
}

impl QpdfCli {
    /// Creates a handle for the `qpdf.exe` at `exe`.
    pub fn new(exe: PathBuf) -> Self {
        Self {
            exe,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Sets the per-run time limit.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn execute(
        &self,
        args: Vec<OsString>,
        password: Option<&Password>,
        had_password: bool,
    ) -> Result<(Output, CheckOutcome), AppError> {
        let stdin = password.map(|p| format!("{}\n", p.expose()).into_bytes());
        let output = run(&self.exe, &args, stdin.as_deref(), self.timeout)?;
        match output.code {
            Some(0) => Ok((output, CheckOutcome::Clean)),
            Some(3) => Ok((output, CheckOutcome::Warnings)),
            _ => Err(map_failure(&output.stderr, had_password)),
        }
    }

    /// Runs `qpdf --check`. Exit code 3 (warnings) passes and is reported as such.
    pub fn check(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<CheckOutcome, AppError> {
        let mut args: Vec<OsString> = Vec::new();
        if password.is_some() {
            args.push("--password-file=-".into());
        }
        args.push("--check".into());
        args.push(path.into());
        self.execute(args, password, password.is_some())
            .map(|(_, outcome)| outcome)
    }
}

impl PdfWriter for QpdfCli {
    /// Copies `pages` into a new, unencrypted document. The pages keep the order given.
    fn copy_pages(
        &self,
        source: &Path,
        password: Option<&Password>,
        pages: &[PageIndex],
        dest: &Path,
    ) -> Result<(), AppError> {
        if pages.is_empty() {
            return Err(domain::DomainError::option("pages", "select at least one page").into());
        }
        let list: Vec<String> = pages
            .iter()
            .map(|p| p.to_number().get().to_string())
            .collect();
        let mut args: Vec<OsString> = Vec::new();
        if password.is_some() {
            args.push("--password-file=-".into());
        }
        args.push("--decrypt".into());
        args.push(source.into());
        args.push("--pages".into());
        args.push(".".into());
        args.push(list.join(",").into());
        args.push("--".into());
        args.push(dest.into());
        self.execute(args, password, password.is_some()).map(|_| ())
    }
}

/// Converts qpdf's complaint into a catalog error. qpdf text is matched, never shown.
fn map_failure(stderr: &str, had_password: bool) -> AppError {
    let text = stderr.to_lowercase();
    if text.contains("invalid password") {
        return if had_password {
            AppError::new(
                ErrorCode::PdfWrongPassword,
                "The password is not correct. Check it and try again.",
            )
        } else {
            AppError::new(
                ErrorCode::PdfEncrypted,
                "This file is password protected. Enter the password to open it.",
            )
        };
    }
    let damaged = [
        "can't find pdf header",
        "not a pdf",
        "unable to find trailer",
        "xref",
        "damaged",
        "end of file",
        "unexpected eof",
        "stream",
    ];
    if damaged.iter().any(|needle| text.contains(needle)) {
        return AppError::new(
            ErrorCode::PdfDamaged,
            "This file is damaged. Try the Repair tool.",
        );
    }
    if text.contains("no such file") || text.contains("cannot open") || text.contains("can't open")
    {
        return AppError::new(
            ErrorCode::PathDenied,
            "The file could not be read. Check that it exists and is not open in another program.",
        );
    }
    AppError::internal("processing the PDF")
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::{minimal_pdf, qpdf_exe};

    fn qpdf() -> QpdfCli {
        QpdfCli::new(qpdf_exe().expect("run scripts/fetch-qpdf.ps1 first"))
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Encrypts a PDF with qpdf itself (AES-256, user password `open`).
    fn encrypt(dir: &tempfile::TempDir, source: &Path) -> PathBuf {
        let out = dir.path().join("locked.pdf");
        let status = std::process::Command::new(qpdf_exe().unwrap())
            .args(["--encrypt", "open", "owner", "256", "--"])
            .arg(source)
            .arg(&out)
            .status()
            .unwrap();
        assert!(status.success());
        out
    }

    #[test]
    fn check_passes_a_valid_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.pdf", &minimal_pdf(2));
        assert_eq!(qpdf().check(&path, None).unwrap(), CheckOutcome::Clean);
    }

    #[test]
    fn check_reports_damage_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let junk = write(&dir, "j.pdf", b"not a pdf");
        assert_eq!(
            qpdf().check(&junk, None).unwrap_err().code,
            ErrorCode::PdfDamaged
        );
        let missing = dir.path().join("none.pdf");
        assert_eq!(
            qpdf().check(&missing, None).unwrap_err().code,
            ErrorCode::PathDenied
        );
    }

    #[test]
    fn copies_selected_pages_in_the_requested_order() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(5));
        let dest = dir.path().join("out.pdf");
        qpdf()
            .copy_pages(&src, None, &[PageIndex::new(3), PageIndex::new(0)], &dest)
            .unwrap();
        assert!(std::fs::read(&dest).unwrap().starts_with(b"%PDF-"));
        assert_eq!(qpdf().check(&dest, None).unwrap(), CheckOutcome::Clean);
        // The output uses compressed streams, so expand it to look at page text.
        let expanded = dir.path().join("expanded.pdf");
        let status = std::process::Command::new(qpdf_exe().unwrap())
            .args(["--qdf", "--object-streams=disable"])
            .arg(&dest)
            .arg(&expanded)
            .status()
            .unwrap();
        assert!(status.success());
        let bytes = std::fs::read(&expanded).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let first = text.find("Page 4 of 5").expect("page 4 present");
        let second = text.find("Page 1 of 5").expect("page 1 present");
        assert!(first < second, "page 4 comes before page 1");
        assert!(!text.contains("Page 2 of 5"));
    }

    #[test]
    fn copying_nothing_is_an_option_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(1));
        let err = qpdf()
            .copy_pages(&src, None, &[], &dir.path().join("o.pdf"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
    }

    #[test]
    fn passwords_travel_on_stdin_and_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write(&dir, "a.pdf", &minimal_pdf(2));
        let locked = encrypt(&dir, &plain);

        assert_eq!(
            qpdf().check(&locked, None).unwrap_err().code,
            ErrorCode::PdfEncrypted
        );
        let wrong = Password::new("nope");
        assert_eq!(
            qpdf().check(&locked, Some(&wrong)).unwrap_err().code,
            ErrorCode::PdfWrongPassword
        );
        let right = Password::new("open");
        assert_eq!(
            qpdf().check(&locked, Some(&right)).unwrap(),
            CheckOutcome::Clean
        );

        let dest = dir.path().join("unlocked-copy.pdf");
        qpdf()
            .copy_pages(&locked, Some(&right), &[PageIndex::new(1)], &dest)
            .unwrap();
        assert_eq!(
            qpdf().check(&dest, None).unwrap(),
            CheckOutcome::Clean,
            "output is unencrypted"
        );
    }

    #[test]
    fn a_hung_process_is_stopped_at_the_timeout() {
        let started = std::time::Instant::now();
        let err = process::run(
            &PathBuf::from(r"C:\Windows\System32\ping.exe"),
            &["-n".into(), "30".into(), "127.0.0.1".into()],
            None,
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::EngineTimeout);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn failure_text_maps_to_catalog_codes() {
        assert_eq!(
            map_failure("qpdf: x.pdf: invalid password", true).code,
            ErrorCode::PdfWrongPassword
        );
        assert_eq!(
            map_failure("qpdf: x.pdf: invalid password", false).code,
            ErrorCode::PdfEncrypted
        );
        assert_eq!(
            map_failure("can't find PDF header", false).code,
            ErrorCode::PdfDamaged
        );
        assert_eq!(
            map_failure("something unexpected", false).code,
            ErrorCode::Internal
        );
    }
}
