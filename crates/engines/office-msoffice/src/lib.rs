//! Microsoft Office adapter (Appendix A rows 19 to 21): Word, PowerPoint and Excel documents
//! become PDFs through the programs installed on this computer.
//!
//! The programs are driven by Windows PowerShell and COM automation, so Quirewise carries no
//! Office code. Each run starts its own hidden instance of the program, opens the document read
//! only with macros switched off, exports a PDF and quits again.
#![forbid(unsafe_code)]

mod proc;
mod script;

use application::AppError;
use application::CancellationToken;
use application::ports::{OfficeConverter, OfficeKind, OfficeRequest};
use domain::tools::office::SheetOrientation;
use domain::{Duplicates, ErrorCode};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

/// Longest one conversion may take.
const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Most sheets a workbook is searched for when a range such as `odd` is expanded.
const MAX_SHEETS: u32 = 256;

/// [`OfficeConverter`] over Microsoft Office.
#[derive(Debug, Clone, Default)]
pub struct MsOffice {
    /// The PowerShell to run, found from `SystemRoot` when not set.
    powershell: Option<PathBuf>,
}

impl MsOffice {
    /// Creates the adapter.
    pub fn new() -> Self {
        Self::default()
    }

    fn powershell(&self) -> PathBuf {
        self.powershell.clone().unwrap_or_else(|| {
            let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            PathBuf::from(root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe")
        })
    }
}

/// The COM name of the program that opens documents of `kind`.
const fn prog_id(kind: OfficeKind) -> &'static str {
    match kind {
        OfficeKind::Word => "Word.Application",
        OfficeKind::Slides => "PowerPoint.Application",
        OfficeKind::Sheet => "Excel.Application",
    }
}

fn registered(kind: OfficeKind) -> bool {
    let key = format!("HKCR\\{}\\CLSID", prog_id(kind));
    let mut command = Command::new("reg.exe");
    command.args(["query", &key]);
    proc::quiet(&mut command);
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

impl OfficeConverter for MsOffice {
    fn name(&self) -> &'static str {
        "Microsoft Office"
    }

    fn is_available(&self, kind: OfficeKind) -> bool {
        static FOUND: OnceLock<[bool; 3]> = OnceLock::new();
        let found = FOUND.get_or_init(|| {
            [
                registered(OfficeKind::Word),
                registered(OfficeKind::Slides),
                registered(OfficeKind::Sheet),
            ]
        });
        found[match kind {
            OfficeKind::Word => 0,
            OfficeKind::Slides => 1,
            OfficeKind::Sheet => 2,
        }]
    }

    fn convert(
        &self,
        source: &Path,
        request: &OfficeRequest,
        _scratch: &Path,
        dest: &Path,
        cancel: &CancellationToken,
    ) -> Result<(), AppError> {
        let sheets = if request.sheets == domain::PageRange::all() {
            String::new()
        } else {
            request
                .sheets
                .resolve(MAX_SHEETS, Duplicates::Allow)?
                .pages()
                .iter()
                .map(|p| (p.get() + 1).to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let orientation = match request.orientation {
            SheetOrientation::Auto => "auto",
            SheetOrientation::Portrait => "portrait",
            SheetOrientation::Landscape => "landscape",
        };
        let mut command = Command::new(self.powershell());
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-OutputFormat",
                "Text",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
                &script::encoded(),
            ])
            .env("QW_KIND", prog_id(request.kind))
            .env("QW_IN", source)
            .env("QW_OUT", dest)
            .env(
                "QW_HIDDEN",
                if request.include_hidden_slides {
                    "-1"
                } else {
                    "0"
                },
            )
            .env("QW_SHEETS", sheets)
            .env("QW_FIT", if request.fit_to_page_width { "1" } else { "0" })
            .env("QW_ORIENT", orientation);
        let outcome = proc::run(command, TIMEOUT, cancel)?;
        if outcome.success && dest.is_file() {
            return Ok(());
        }
        Err(failure(&outcome.message))
    }
}

/// The error for a conversion that did not produce a file, with what the script said.
fn failure(message: &str) -> AppError {
    let detail = message.trim();
    if let Some(rest) = detail.strip_prefix("QW:") {
        return AppError::new(ErrorCode::OptionInvalid, rest.trim().to_owned());
    }
    AppError::new(
        ErrorCode::OutputVerifyFailed,
        if detail.is_empty() {
            "Microsoft Office could not convert this file. Open it in Office to check that it is not damaged or password protected.".to_owned()
        } else {
            format!("Microsoft Office could not convert this file: {detail}")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_messages_that_start_with_a_marker_are_option_problems() {
        let e = failure("QW: The workbook has only 3 sheets.");
        assert_eq!(e.code, ErrorCode::OptionInvalid);
        assert!(e.message.contains("only 3 sheets"));
    }

    #[test]
    fn other_failures_say_what_office_reported() {
        let e = failure("The file is corrupt");
        assert_eq!(e.code, ErrorCode::OutputVerifyFailed);
        assert!(e.message.contains("corrupt"));
        assert!(failure("").message.contains("damaged"));
    }

    // The tests below run Office itself. They do nothing on a computer without it.
    fn convert(name: &str, bytes: &[u8], request: OfficeRequest) -> Option<Result<u32, AppError>> {
        let engine = MsOffice::new();
        if !engine.is_available(request.kind) {
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join(name);
        std::fs::write(&source, bytes).unwrap();
        let dest = dir.path().join("out.pdf");
        let result = engine
            .convert(
                &source,
                &request,
                dir.path(),
                &dest,
                &CancellationToken::new(),
            )
            .map(|()| lopdf::Document::load(&dest).unwrap().get_pages().len() as u32);
        Some(result)
    }

    #[test]
    fn word_documents_become_pdfs_with_their_pages() {
        let docx = test_support::office::docx_with_pages(&["one", "two", "three"]);
        let Some(result) = convert("a.docx", &docx, OfficeRequest::plain(OfficeKind::Word)) else {
            return;
        };
        assert_eq!(result.unwrap(), 3);
    }

    #[test]
    fn a_damaged_word_file_is_reported_not_hung_on() {
        let Some(result) = convert(
            "bad.docx",
            b"PK\x03\x04 not really a document",
            OfficeRequest::plain(OfficeKind::Word),
        ) else {
            return;
        };
        assert_eq!(result.unwrap_err().code, ErrorCode::OutputVerifyFailed);
    }

    fn workbook() -> Vec<u8> {
        test_support::office::xlsx_with_sheets(&[
            ("First", vec![vec!["a", "b"], vec!["c", "d"]]),
            ("Second", vec![vec!["e"]]),
            ("Third", vec![vec!["f"]]),
        ])
    }

    #[test]
    fn every_sheet_is_printed_unless_some_are_chosen() {
        let all = OfficeRequest {
            fit_to_page_width: true,
            ..OfficeRequest::plain(OfficeKind::Sheet)
        };
        let Some(result) = convert("a.xlsx", &workbook(), all) else {
            return;
        };
        assert_eq!(result.unwrap(), 3);
        let some = OfficeRequest {
            sheets: domain::PageRange::parse("1,3").unwrap(),
            ..OfficeRequest::plain(OfficeKind::Sheet)
        };
        assert_eq!(convert("a.xlsx", &workbook(), some).unwrap().unwrap(), 2);
        let none = OfficeRequest {
            sheets: domain::PageRange::parse("9").unwrap(),
            ..OfficeRequest::plain(OfficeKind::Sheet)
        };
        let err = convert("a.xlsx", &workbook(), none).unwrap().unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        assert!(err.message.contains("only 3 sheets"), "{}", err.message);
    }

    /// Makes a presentation of three blank slides, the second one hidden, with PowerPoint itself:
    /// a PPTX is too involved to write by hand.
    fn presentation(dir: &Path) -> PathBuf {
        let path = dir.join("deck.pptx");
        let script = "$ErrorActionPreference='Stop'; $a=New-Object -ComObject PowerPoint.Application; \
$p=$a.Presentations.Add(0); 1..3 | ForEach-Object { [void]$p.Slides.Add($_, 12) }; \
$p.Slides.Item(2).SlideShowTransition.Hidden = -1; $p.SaveAs($env:QW_DECK); $p.Close(); $a.Quit()";
        let status = Command::new(MsOffice::new().powershell())
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("QW_DECK", &path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "PowerPoint could not make the test deck");
        path
    }

    #[test]
    fn hidden_slides_are_left_out_unless_asked_for() {
        let engine = MsOffice::new();
        if !engine.is_available(OfficeKind::Slides) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let deck = presentation(dir.path());
        let pages = |hidden: bool| {
            let dest = dir.path().join(format!("out-{hidden}.pdf"));
            let request = OfficeRequest {
                include_hidden_slides: hidden,
                ..OfficeRequest::plain(OfficeKind::Slides)
            };
            engine
                .convert(
                    &deck,
                    &request,
                    dir.path(),
                    &dest,
                    &CancellationToken::new(),
                )
                .unwrap();
            lopdf::Document::load(&dest).unwrap().get_pages().len()
        };
        assert_eq!(pages(false), 2);
        assert_eq!(pages(true), 3);
    }

    #[test]
    fn every_kind_has_a_com_name() {
        assert_eq!(prog_id(OfficeKind::Word), "Word.Application");
        assert_eq!(prog_id(OfficeKind::Slides), "PowerPoint.Application");
        assert_eq!(prog_id(OfficeKind::Sheet), "Excel.Application");
    }
}
