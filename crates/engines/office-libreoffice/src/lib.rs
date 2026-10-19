//! LibreOffice adapter (Appendix A rows 19 to 21): documents become PDFs through LibreOffice's
//! headless mode.
//!
//! LibreOffice is an on-demand component (ADR-0005). A copy installed on the computer is used too,
//! so people who already have it need no download. Every run gets its own empty profile folder,
//! which keeps it apart from the person's own LibreOffice settings and from other runs.
#![forbid(unsafe_code)]

mod proc;
pub mod xlsx;

use application::AppError;
use application::CancellationToken;
use application::ports::{ComponentStore, OfficeConverter, OfficeKind, OfficeRequest};
use domain::{Duplicates, ErrorCode};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Longest one conversion may take.
const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Most sheets a workbook is searched for when a range such as `odd` is expanded.
const MAX_SHEETS: u32 = 256;
/// Id of the component that holds LibreOffice.
pub const COMPONENT_ID: &str = "libreoffice";

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// [`OfficeConverter`] over LibreOffice.
#[derive(Clone)]
pub struct LibreOffice {
    components: Option<Arc<dyn ComponentStore>>,
    /// A program to use instead of looking for LibreOffice, for tests.
    fixed: Option<PathBuf>,
}

impl LibreOffice {
    /// Creates the adapter. `components` is where an installed LibreOffice component is found.
    pub fn new(components: Option<Arc<dyn ComponentStore>>) -> Self {
        Self {
            components,
            fixed: None,
        }
    }

    /// An adapter that always runs `program`.
    pub fn with_program(program: PathBuf) -> Self {
        Self {
            components: None,
            fixed: Some(program),
        }
    }

    /// The `soffice.exe` to run: the component first, then a copy installed on the computer.
    pub fn locate(&self) -> Option<PathBuf> {
        if let Some(fixed) = &self.fixed {
            return fixed.is_file().then(|| fixed.clone());
        }
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(dir) = self
            .components
            .as_ref()
            .and_then(|c| c.locate(COMPONENT_ID))
        {
            candidates.push(dir.join("program").join("soffice.exe"));
            candidates.push(dir.join("LibreOffice").join("program").join("soffice.exe"));
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(root) = std::env::var_os(var) {
                candidates.push(
                    PathBuf::from(root)
                        .join("LibreOffice")
                        .join("program")
                        .join("soffice.exe"),
                );
            }
        }
        candidates.into_iter().find(|p| p.is_file())
    }
}

/// The filter that turns a document of `kind` into a PDF.
fn filter(request: &OfficeRequest) -> String {
    match request.kind {
        OfficeKind::Word => "pdf:writer_pdf_Export".to_owned(),
        OfficeKind::Slides => {
            if request.include_hidden_slides {
                "pdf:impress_pdf_Export:{\"ExportHiddenSlides\":{\"type\":\"boolean\",\"value\":\"true\"}}"
                    .to_owned()
            } else {
                "pdf:impress_pdf_Export".to_owned()
            }
        }
        OfficeKind::Sheet => "pdf:calc_pdf_Export".to_owned(),
    }
}

/// A `file:///` address for a folder, as LibreOffice's `-env:UserInstallation` wants it.
fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_start_matches("//?/");
    let encoded: String = text
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_owned(),
            '%' => "%25".to_owned(),
            '#' => "%23".to_owned(),
            _ => c.to_string(),
        })
        .collect();
    format!("file:///{}", encoded.trim_start_matches('/'))
}

/// Settings for a workbook, when the request asks for any and the file is an `.xlsx` package.
fn workbook_settings(request: &OfficeRequest) -> Result<Option<xlsx::Settings>, AppError> {
    let sheets = if request.sheets == domain::PageRange::all() {
        None
    } else {
        Some(
            request
                .sheets
                .resolve(MAX_SHEETS, Duplicates::Allow)?
                .pages()
                .iter()
                .map(|p| p.get() + 1)
                .collect(),
        )
    };
    let settings = xlsx::Settings {
        sheets,
        fit_to_width: request.fit_to_page_width,
        orientation: request.orientation,
    };
    Ok((!settings.is_empty()).then_some(settings))
}

fn is_xlsx(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| ["xlsx", "xlsm"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

impl OfficeConverter for LibreOffice {
    fn name(&self) -> &'static str {
        "LibreOffice"
    }

    fn is_available(&self, _: OfficeKind) -> bool {
        self.locate().is_some()
    }

    fn convert(
        &self,
        source: &Path,
        request: &OfficeRequest,
        scratch: &Path,
        dest: &Path,
        cancel: &CancellationToken,
    ) -> Result<(), AppError> {
        let program = self.locate().ok_or_else(|| {
            AppError::new(
                ErrorCode::ComponentMissing,
                "LibreOffice is not installed. Open Settings, then Components, to install it.",
            )
        })?;
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let work = scratch.join(format!("qw-lo-{}-{n}", std::process::id()));
        let result = (|| {
            fs::create_dir_all(&work)
                .map_err(|e| AppError::from_io("prepare the conversion", &e))?;
            let outdir = work.join("out");
            fs::create_dir_all(&outdir)
                .map_err(|e| AppError::from_io("prepare the conversion", &e))?;
            let profile = work.join("profile");

            // A workbook gets its sheet settings written into a copy; other files go as they are.
            let mut input = source.to_path_buf();
            if request.kind == OfficeKind::Sheet
                && let Some(settings) = workbook_settings(request)?
            {
                if is_xlsx(source) {
                    let bytes =
                        fs::read(source).map_err(|e| AppError::from_io("read the workbook", &e))?;
                    let patched = xlsx::apply(&bytes, &settings)?;
                    input = work.join("input.xlsx");
                    fs::write(&input, patched)
                        .map_err(|e| AppError::from_io("prepare the workbook", &e))?;
                } else if settings.sheets.is_some() {
                    return Err(AppError::new(
                        ErrorCode::OptionInvalid,
                        "Choosing sheets needs an .xlsx file with LibreOffice. Save the workbook as .xlsx, or use Microsoft Office.",
                    ));
                }
            }

            let mut command = Command::new(&program);
            command
                .args([
                    "--headless",
                    "--norestore",
                    "--nolockcheck",
                    "--nodefault",
                    "--nologo",
                    "--nofirststartwizard",
                ])
                .arg(format!("-env:UserInstallation={}", file_url(&profile)))
                .arg("--convert-to")
                .arg(filter(request))
                .arg("--outdir")
                .arg(&outdir)
                .arg(&input);
            let outcome = proc::run(command, TIMEOUT, cancel)?;
            let stem = input
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let produced = outdir.join(format!("{stem}.pdf"));
            if !outcome.success || !produced.is_file() {
                let said = outcome
                    .message
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .map(|l| l.chars().take(200).collect::<String>());
                return Err(AppError::new(
                    ErrorCode::OutputVerifyFailed,
                    match said {
                        Some(detail) => format!("LibreOffice could not convert this file ({detail}). Open it in LibreOffice to check that it is not damaged or password protected."),
                        None => "LibreOffice could not convert this file. Open it in LibreOffice to check that it is not damaged or password protected.".to_owned(),
                    },
                ));
            }
            fs::copy(&produced, dest)
                .map(|_| ())
                .map_err(|e| AppError::from_io("save the converted file", &e))
        })();
        let _ = fs::remove_dir_all(&work);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf_bytes() -> Vec<u8> {
        b"%PDF-1.4 fake".to_vec()
    }

    /// A stand-in for `soffice.exe`: a batch file that copies a fixed file to the output folder
    /// under the input's name, and records how it was called.
    fn fake_program(dir: &Path, fixture: &Path, log: &Path) -> PathBuf {
        let script = format!(
            "@echo off\r\nsetlocal enabledelayedexpansion\r\nset OUT=\r\nset LAST=\r\n:next\r\nif \"%~1\"==\"\" goto done\r\n>>\"{log}\" echo %~1\r\nif \"%~1\"==\"--outdir\" (\r\n  set OUT=%~2\r\n  >>\"{log}\" echo %~2\r\n  shift\r\n  shift\r\n  goto next\r\n)\r\nset LAST=%~1\r\nshift\r\ngoto next\r\n:done\r\nfor %%I in (\"!LAST!\") do set NAME=%%~nI\r\ncopy /Y \"{fixture}\" \"!OUT!\\!NAME!.pdf\" >nul\r\n",
            log = log.display(),
            fixture = fixture.display()
        );
        let path = dir.join("soffice.cmd");
        fs::write(&path, script).unwrap();
        path
    }

    fn setup() -> (tempfile::TempDir, LibreOffice, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let fixture = dir.path().join("fixture.pdf");
        fs::write(&fixture, pdf_bytes()).unwrap();
        let log = dir.path().join("calls.txt");
        let program = fake_program(dir.path(), &fixture, &log);
        (dir, LibreOffice::with_program(program), log)
    }

    fn run(
        engine: &LibreOffice,
        dir: &Path,
        name: &str,
        bytes: &[u8],
        request: &OfficeRequest,
    ) -> Result<Vec<u8>, AppError> {
        let source = dir.join(name);
        fs::write(&source, bytes).unwrap();
        let dest = dir.join("result.pdf");
        engine.convert(&source, request, dir, &dest, &CancellationToken::new())?;
        Ok(fs::read(&dest).unwrap())
    }

    #[test]
    fn the_document_goes_to_headless_libreoffice_with_its_own_profile() {
        let (dir, engine, log) = setup();
        let out = run(
            &engine,
            dir.path(),
            "report.docx",
            b"x",
            &OfficeRequest::plain(OfficeKind::Word),
        )
        .unwrap();
        assert_eq!(out, pdf_bytes());
        let calls = fs::read_to_string(&log).unwrap();
        for expected in [
            "--headless",
            "--norestore",
            "--convert-to",
            "pdf:writer_pdf_Export",
            "--outdir",
        ] {
            assert!(calls.contains(expected), "{expected} in {calls}");
        }
        assert!(calls.contains("-env:UserInstallation=file:///"), "{calls}");
        // Nothing is left behind in the scratch folder.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("qw-lo-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn hidden_slides_are_a_filter_option() {
        let (dir, engine, log) = setup();
        let request = OfficeRequest {
            include_hidden_slides: true,
            ..OfficeRequest::plain(OfficeKind::Slides)
        };
        run(&engine, dir.path(), "deck.pptx", b"x", &request).unwrap();
        let calls = fs::read_to_string(&log).unwrap();
        // A batch file shows the quotes of the argument doubled; the program itself gets them as is.
        assert!(
            calls.contains("impress_pdf_Export:{") && calls.contains("ExportHiddenSlides"),
            "{calls}"
        );
    }

    #[test]
    fn a_workbook_is_prepared_before_it_is_handed_over() {
        let (dir, engine, log) = setup();
        let book = test_support::office::xlsx_with_sheets(&[
            ("A", vec![vec!["1"]]),
            ("B", vec![vec!["2"]]),
        ]);
        let request = OfficeRequest {
            sheets: domain::PageRange::parse("2").unwrap(),
            fit_to_page_width: true,
            ..OfficeRequest::plain(OfficeKind::Sheet)
        };
        run(&engine, dir.path(), "book.xlsx", &book, &request).unwrap();
        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("calc_pdf_Export"));
        assert!(
            calls.contains("input.xlsx"),
            "the prepared copy is converted: {calls}"
        );
    }

    #[test]
    fn choosing_sheets_of_an_old_workbook_is_refused_clearly() {
        let (dir, engine, _) = setup();
        let request = OfficeRequest {
            sheets: domain::PageRange::parse("1").unwrap(),
            ..OfficeRequest::plain(OfficeKind::Sheet)
        };
        let err = run(&engine, dir.path(), "old.xls", b"x", &request).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        assert!(err.message.contains(".xlsx"));
    }

    #[test]
    fn a_missing_program_is_a_missing_component() {
        let engine = LibreOffice::with_program(PathBuf::from("Z:\\nowhere\\soffice.exe"));
        assert!(!engine.is_available(OfficeKind::Word));
        let dir = tempfile::tempdir().unwrap();
        let err = run(
            &engine,
            dir.path(),
            "a.docx",
            b"x",
            &OfficeRequest::plain(OfficeKind::Word),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }

    #[test]
    fn a_program_that_makes_no_pdf_is_a_failed_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("soffice.cmd");
        fs::write(&program, "@echo off\r\nexit /b 0\r\n").unwrap();
        let engine = LibreOffice::with_program(program);
        let err = run(
            &engine,
            dir.path(),
            "a.docx",
            b"x",
            &OfficeRequest::plain(OfficeKind::Word),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::OutputVerifyFailed);
    }

    #[test]
    fn cancelling_stops_the_program() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("soffice.cmd");
        fs::write(&program, "@echo off\r\nping -n 30 127.0.0.1 >nul\r\n").unwrap();
        let engine = LibreOffice::with_program(program);
        let source = dir.path().join("a.docx");
        fs::write(&source, b"x").unwrap();
        let cancel = CancellationToken::new();
        let stopper = cancel.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            stopper.cancel();
        });
        let started = std::time::Instant::now();
        let err = engine
            .convert(
                &source,
                &OfficeRequest::plain(OfficeKind::Word),
                dir.path(),
                &dir.path().join("o.pdf"),
                &cancel,
            )
            .unwrap_err();
        handle.join().unwrap();
        assert_eq!(err.code, ErrorCode::JobCancelled);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn folder_addresses_escape_what_a_url_cannot_hold() {
        assert_eq!(file_url(Path::new("C:\\a b\\c")), "file:///C:/a%20b/c");
        assert_eq!(file_url(Path::new("C:\\100%\\x")), "file:///C:/100%25/x");
    }
}
