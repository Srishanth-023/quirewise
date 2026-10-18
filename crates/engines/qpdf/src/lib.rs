//! qpdf adapter: structural checks and page copying through the `qpdf` command line sidecar.
//!
//! qpdf runs as a separate process and is never linked in. Passwords are passed on standard
//! input (`--password-file=-`), never as arguments or temp files, so they cannot show up in a
//! process list or on disk.
#![forbid(unsafe_code)]

mod assemble;
mod blank;
mod process;
mod security;

use application::AppError;
use application::Password;
use application::ports::{AssemblyPlan, PdfWriter};
use domain::tools::common::RightAngle;
use domain::{ErrorCode, PageIndex};
use process::{Output, run};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Default time limit for one qpdf run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Makes a path absolute without touching the file system. qpdf runs with its own folder as the
/// working directory, so relative paths would point at the wrong place.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

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

    pub(crate) fn execute(
        &self,
        args: Vec<OsString>,
        password: Option<&Password>,
        had_password: bool,
    ) -> Result<(Output, CheckOutcome), AppError> {
        let stdin = password.map(|p| format!("{}\n", p.expose()).into_bytes());
        let output = run(&self.exe, &args, stdin.as_deref(), self.timeout)?;
        Self::finish(output, had_password)
    }

    /// Runs qpdf with its arguments read from standard input (`@-`), one per line. Used when
    /// the arguments themselves contain secrets.
    pub(crate) fn execute_args_from_stdin(
        &self,
        lines: &[String],
        had_password: bool,
    ) -> Result<(Output, CheckOutcome), AppError> {
        let mut text = lines.join("\n");
        text.push('\n');
        let args: Vec<OsString> = vec!["@-".into()];
        let output = run(&self.exe, &args, Some(text.as_bytes()), self.timeout)?;
        Self::finish(output, had_password)
    }

    fn finish(output: Output, had_password: bool) -> Result<(Output, CheckOutcome), AppError> {
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
        args.push(absolute(path).into_os_string());
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
        args.push(absolute(source).into_os_string());
        args.push("--pages".into());
        args.push(".".into());
        args.push(list.join(",").into());
        args.push("--".into());
        args.push(absolute(dest).into_os_string());
        self.execute(args, password, password.is_some()).map(|_| ())
    }

    /// Turns the given pages further and writes everything else unchanged. The output is
    /// unencrypted.
    fn rotate(
        &self,
        source: &Path,
        password: Option<&Password>,
        rotations: &[(PageIndex, RightAngle)],
        dest: &Path,
    ) -> Result<(), AppError> {
        let numbered: Vec<(u32, RightAngle)> = rotations
            .iter()
            .map(|(page, angle)| (page.get() + 1, *angle))
            .collect();
        let mut args: Vec<OsString> = Vec::new();
        if password.is_some() {
            args.push("--password-file=-".into());
        }
        args.push("--decrypt".into());
        args.push(absolute(source).into_os_string());
        args.push(absolute(dest).into_os_string());
        args.extend(
            assemble::rotate_args(&numbered)
                .into_iter()
                .map(OsString::from),
        );
        self.execute(args, password, password.is_some()).map(|_| ())
    }

    /// Builds a document from several sources and blank pages in one run.
    fn assemble(&self, plan: &AssemblyPlan, temp_dir: &Path, dest: &Path) -> Result<(), AppError> {
        assemble::assemble(self, plan, temp_dir, dest)
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

    fn npages(path: &Path) -> usize {
        let out = std::process::Command::new(qpdf_exe().unwrap())
            .arg("--show-npages")
            .arg(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    /// The document with object streams expanded, so page content can be searched as text.
    fn expanded(dir: &tempfile::TempDir, path: &Path) -> String {
        let out = dir.path().join(format!(
            "expanded-{}.pdf",
            path.file_name().unwrap().to_string_lossy()
        ));
        let status = std::process::Command::new(qpdf_exe().unwrap())
            .args(["--qdf", "--object-streams=disable"])
            .arg(path)
            .arg(&out)
            .status()
            .unwrap();
        assert!(status.success());
        String::from_utf8_lossy(&std::fs::read(out).unwrap()).into_owned()
    }

    fn source(path: &Path) -> application::ports::SourceDoc {
        application::ports::SourceDoc {
            path: path.to_path_buf(),
            password: None,
        }
    }

    fn src_page(source: usize, page: u32, rotate: RightAngle) -> application::ports::PlannedPage {
        application::ports::PlannedPage::Source {
            source,
            page: PageIndex::new(page),
            rotate,
        }
    }

    #[test]
    fn assemble_interleaves_sources_and_rotates_output_pages() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(
            &dir,
            "a.pdf",
            &test_support::pdfs::pdf_with_texts(&["A1", "A2", "A3"]),
        );
        let b = write(
            &dir,
            "b.pdf",
            &test_support::pdfs::pdf_with_texts(&["B1", "B2"]),
        );
        let dest = dir.path().join("mixed.pdf");
        let plan = application::ports::AssemblyPlan {
            sources: vec![source(&a), source(&b)],
            pages: vec![
                src_page(0, 0, RightAngle::Deg0),
                src_page(1, 0, RightAngle::Deg0),
                src_page(0, 1, RightAngle::Deg90),
                src_page(1, 1, RightAngle::Deg0),
            ],
        };
        qpdf().assemble(&plan, dir.path(), &dest).unwrap();
        assert_eq!(npages(&dest), 4);
        assert_eq!(qpdf().check(&dest, None).unwrap(), CheckOutcome::Clean);
        let text = expanded(&dir, &dest);
        let order: Vec<usize> = ["(A1)", "(B1)", "(A2)", "(B2)"]
            .iter()
            .map(|t| text.find(t).unwrap())
            .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "pages are interleaved: {order:?}"
        );
        assert!(!text.contains("(A3)"));
        assert_eq!(
            text.matches("/Rotate 90").count(),
            1,
            "only the third page is turned"
        );
    }

    #[test]
    fn assemble_inserts_blank_pages_and_duplicates_pages() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(
            &dir,
            "a.pdf",
            &test_support::pdfs::pdf_with_texts(&["A1", "A2"]),
        );
        let dest = dir.path().join("out.pdf");
        let plan = application::ports::AssemblyPlan {
            sources: vec![source(&a)],
            pages: vec![
                src_page(0, 0, RightAngle::Deg0),
                application::ports::PlannedPage::Blank {
                    width_pt: 300.0,
                    height_pt: 400.0,
                },
                src_page(0, 0, RightAngle::Deg0),
                application::ports::PlannedPage::Blank {
                    width_pt: 300.0,
                    height_pt: 400.0,
                },
                src_page(0, 1, RightAngle::Deg0),
            ],
        };
        qpdf().assemble(&plan, dir.path(), &dest).unwrap();
        assert_eq!(npages(&dest), 5);
        let text = expanded(&dir, &dest);
        // A duplicated page is a second page object over the same content stream.
        let pages = text.matches("/Type /Page").count() - text.matches("/Type /Pages").count();
        assert_eq!(pages, 5);
        assert!(text.contains("(A1)") && text.contains("(A2)"));
        let compact = text
            .split_whitespace()
            .collect::<String>()
            .replace(".00", "");
        assert_eq!(
            compact.matches("/MediaBox[00300400]").count(),
            2,
            "two blank pages"
        );
    }

    #[test]
    fn assemble_reads_password_protected_sources_without_a_password_on_the_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write(
            &dir,
            "p.pdf",
            &test_support::pdfs::pdf_with_texts(&["P1", "P2"]),
        );
        let locked = dir.path().join("locked.pdf");
        test_support::encrypt_pdf(&plain, &locked, "open");
        let other = write(&dir, "o.pdf", &test_support::pdfs::pdf_with_texts(&["O1"]));
        let dest = dir.path().join("out.pdf");
        let plan = application::ports::AssemblyPlan {
            sources: vec![
                application::ports::SourceDoc {
                    path: locked.clone(),
                    password: Some(Password::new("open")),
                },
                source(&other),
            ],
            pages: vec![
                src_page(0, 1, RightAngle::Deg0),
                src_page(1, 0, RightAngle::Deg0),
            ],
        };
        qpdf().assemble(&plan, dir.path(), &dest).unwrap();
        assert_eq!(npages(&dest), 2);
        assert_eq!(
            qpdf().check(&dest, None).unwrap(),
            CheckOutcome::Clean,
            "output is not encrypted"
        );

        let wrong = application::ports::AssemblyPlan {
            sources: vec![application::ports::SourceDoc {
                path: locked,
                password: Some(Password::new("bad")),
            }],
            pages: vec![src_page(0, 0, RightAngle::Deg0)],
        };
        let err = qpdf()
            .assemble(&wrong, dir.path(), &dir.path().join("x.pdf"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PdfWrongPassword);
    }

    #[test]
    fn assemble_cleans_up_scratch_files_and_handles_spaces_in_paths() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("my files (copy)");
        std::fs::create_dir_all(&folder).unwrap();
        let a = folder.join("in put.pdf");
        std::fs::write(&a, test_support::pdfs::pdf_with_texts(&["A1"])).unwrap();
        let scratch = dir.path().join("scratch");
        let dest = folder.join("out (merged).pdf");
        let plan = application::ports::AssemblyPlan {
            sources: vec![source(&a)],
            pages: vec![
                src_page(0, 0, RightAngle::Deg0),
                application::ports::PlannedPage::Blank {
                    width_pt: 100.0,
                    height_pt: 100.0,
                },
            ],
        };
        qpdf().assemble(&plan, &scratch, &dest).unwrap();
        assert_eq!(npages(&dest), 2);
        assert_eq!(
            std::fs::read_dir(&scratch).unwrap().count(),
            0,
            "no scratch files remain"
        );
    }

    #[test]
    fn assemble_refuses_an_empty_plan() {
        let dir = tempfile::tempdir().unwrap();
        let plan = application::ports::AssemblyPlan {
            sources: vec![],
            pages: vec![],
        };
        let err = qpdf()
            .assemble(&plan, dir.path(), &dir.path().join("x.pdf"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
    }

    #[test]
    fn rotate_turns_chosen_pages_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(
            &dir,
            "a.pdf",
            &test_support::pdfs::pdf_with_texts(&["A1", "A2", "A3"]),
        );
        let dest = dir.path().join("r.pdf");
        qpdf()
            .rotate(
                &a,
                None,
                &[
                    (PageIndex::new(1), RightAngle::Deg90),
                    (PageIndex::new(2), RightAngle::Deg180),
                ],
                &dest,
            )
            .unwrap();
        assert_eq!(npages(&dest), 3);
        let text = expanded(&dir, &dest);
        assert_eq!(text.matches("/Rotate 90").count(), 1);
        assert_eq!(text.matches("/Rotate 180").count(), 1);
        assert_eq!(text.matches("/Rotate 0").count(), 1);
    }
}
