//! Web pages to PDF (Appendix A row 22), printed by Microsoft Edge.
//!
//! Edge is the engine of the WebView2 runtime, installed with Windows 11 and with current Windows
//! 10. It is started headless with an empty profile and driven over the Chrome DevTools protocol on
//! a local port: the page loads, then `Page.printToPDF` writes the paper size, margins, scale and
//! backgrounds the options ask for. Quirewise carries no browser code of its own.
//!
//! Pages and their pictures, styles and scripts only reach the internet when the person agreed:
//! otherwise every request that is not a local file, data or blob address is stopped before it
//! leaves the computer.
#![forbid(unsafe_code)]

mod cdp;
mod proc;

use application::AppError;
use application::CancellationToken;
use application::ports::{HtmlConverter, HtmlSource};
use domain::ErrorCode;
use domain::tools::HtmlToPdfOptions;
use domain::tools::office::HtmlOrientation;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::cdp::PrintJob;

/// Longest the browser may take to start and report its port.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// Millimeters in an inch.
const MM_PER_INCH: f32 = 25.4;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// [`HtmlConverter`] over Microsoft Edge.
#[derive(Debug, Clone, Default)]
pub struct EdgePrinter {
    /// A browser to use instead of looking for Edge, for tests.
    fixed: Option<PathBuf>,
}

impl EdgePrinter {
    /// Creates the adapter.
    pub fn new() -> Self {
        Self::default()
    }

    /// An adapter that always runs `program`.
    pub fn with_program(program: PathBuf) -> Self {
        Self {
            fixed: Some(program),
        }
    }

    /// The `msedge.exe` to run.
    pub fn locate(&self) -> Option<PathBuf> {
        if let Some(fixed) = &self.fixed {
            return fixed.is_file().then(|| fixed.clone());
        }
        ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|root| {
                PathBuf::from(root)
                    .join("Microsoft")
                    .join("Edge")
                    .join("Application")
                    .join("msedge.exe")
            })
            .find(|p| p.is_file())
    }
}

/// A `file:///` address for a path on this computer.
fn file_address(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_start_matches("//?/");
    let encoded: String = text
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_owned(),
            '%' => "%25".to_owned(),
            '#' => "%23".to_owned(),
            '?' => "%3F".to_owned(),
            _ => c.to_string(),
        })
        .collect();
    format!("file:///{}", encoded.trim_start_matches('/'))
}

/// Paper and print settings as the DevTools protocol takes them.
fn print_parameters(options: &HtmlToPdfOptions) -> Result<serde_json::Value, AppError> {
    // The page turn is made by swapping the paper sides, so a page that asks for its own size in
    // CSS is not forced back to the one chosen here.
    let (width, height) = options.paper_inches()?;
    let margin = options.margin_mm / MM_PER_INCH;
    Ok(serde_json::json!({
        "landscape": false,
        "displayHeaderFooter": options.header_footer,
        "printBackground": options.background_graphics,
        "scale": f64::from(options.scale_percent) / 100.0,
        "paperWidth": width,
        "paperHeight": height,
        "marginTop": margin,
        "marginBottom": margin,
        "marginLeft": margin,
        "marginRight": margin,
        "preferCSSPageSize": false,
        "transferMode": "ReturnAsBase64",
    }))
}

impl HtmlConverter for EdgePrinter {
    fn is_available(&self) -> bool {
        self.locate().is_some()
    }

    fn print(
        &self,
        source: &HtmlSource,
        options: &HtmlToPdfOptions,
        dest: &Path,
        cancel: &CancellationToken,
    ) -> Result<(), AppError> {
        let program = self.locate().ok_or_else(|| {
            AppError::new(
                ErrorCode::ComponentMissing,
                "Microsoft Edge was not found, so web pages cannot be printed. Install Edge or the WebView2 Runtime from Microsoft, then try again.",
            )
        })?;
        let address = match source {
            HtmlSource::File(path) => file_address(path),
            HtmlSource::Url(url) => url.clone(),
        };
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let profile = std::env::temp_dir().join(format!("qw-edge-{}-{n}", std::process::id()));
        let result = (|| {
            fs::create_dir_all(&profile)
                .map_err(|e| AppError::from_io("prepare the web page printer", &e))?;
            let browser = proc::Browser::start(&program, &profile, START_TIMEOUT, cancel)?;
            let job = PrintJob {
                address: &address,
                load_timeout: Duration::from_secs(u64::from(options.load_timeout_s)),
                block_network: !options.allow_network,
                print: print_parameters(options)?,
            };
            let pdf = cdp::print(browser.endpoint(), &job, cancel)?;
            if !pdf.starts_with(b"%PDF-") {
                return Err(AppError::new(
                    ErrorCode::OutputVerifyFailed,
                    "The browser did not produce a PDF for this page.",
                ));
            }
            fs::write(dest, pdf).map_err(|e| AppError::from_io("save the printed page", &e))
        })();
        let _ = fs::remove_dir_all(&profile);
        result
    }
}

/// Which way the paper turns, for messages and tests.
pub const fn is_landscape(options: &HtmlToPdfOptions) -> bool {
    matches!(options.orientation, HtmlOrientation::Landscape)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    fn printer() -> Option<EdgePrinter> {
        let p = EdgePrinter::new();
        p.is_available().then_some(p)
    }

    fn pages(path: &Path) -> Vec<(f32, f32)> {
        let doc = lopdf::Document::load(path).unwrap();
        doc.get_pages()
            .values()
            .map(|id| {
                let media = doc.get_dictionary(*id).unwrap().get(b"MediaBox").unwrap();
                let v: Vec<f32> = media
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| o.as_float().unwrap())
                    .collect();
                (v[2] - v[0], v[3] - v[1])
            })
            .collect()
    }

    fn page_file(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("page.html");
        fs::write(
            &path,
            format!("<!doctype html><html><head><title>T</title></head><body>{body}</body></html>"),
        )
        .unwrap();
        path
    }

    fn print_file(
        printer: &EdgePrinter,
        source: &Path,
        options: &HtmlToPdfOptions,
        dest: &Path,
    ) -> Result<(), AppError> {
        printer.print(
            &HtmlSource::File(source.to_path_buf()),
            options,
            dest,
            &CancellationToken::new(),
        )
    }

    #[test]
    fn a_local_page_prints_on_the_paper_asked_for() {
        let Some(printer) = printer() else { return };
        let dir = tempfile::tempdir().unwrap();
        let source = page_file(dir.path(), "<h1>Hello</h1><p>World</p>");
        let dest = dir.path().join("a4.pdf");
        print_file(&printer, &source, &HtmlToPdfOptions::default(), &dest).unwrap();
        let sizes = pages(&dest);
        assert_eq!(sizes.len(), 1);
        assert!(
            (sizes[0].0 - 595.3).abs() < 1.0 && (sizes[0].1 - 841.9).abs() < 1.0,
            "{sizes:?}"
        );

        let landscape = HtmlToPdfOptions {
            orientation: HtmlOrientation::Landscape,
            ..HtmlToPdfOptions::default()
        };
        let dest = dir.path().join("land.pdf");
        print_file(&printer, &source, &landscape, &dest).unwrap();
        let sizes = pages(&dest);
        assert!(sizes[0].0 > sizes[0].1, "{sizes:?}");
        assert!(is_landscape(&landscape));
    }

    #[test]
    fn a_long_page_flows_onto_more_pages_and_the_scale_changes_that() {
        let Some(printer) = printer() else { return };
        let dir = tempfile::tempdir().unwrap();
        let body: String = (0..120)
            .map(|i| format!("<p>Line number {i}</p>"))
            .collect();
        let source = page_file(dir.path(), &body);
        let normal = dir.path().join("n.pdf");
        print_file(&printer, &source, &HtmlToPdfOptions::default(), &normal).unwrap();
        let small = dir.path().join("s.pdf");
        let options = HtmlToPdfOptions {
            scale_percent: 50,
            ..HtmlToPdfOptions::default()
        };
        print_file(&printer, &source, &options, &small).unwrap();
        assert!(pages(&normal).len() > pages(&small).len());
        assert!(!pages(&small).is_empty());
    }

    /// A web server that only counts the requests it gets.
    fn counting_server() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buffer = [0u8; 2048];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 28\r\nConnection: close\r\n\r\n<html><body>Online</body></html>"[..].as_ref(),
                );
            }
        });
        (format!("http://127.0.0.1:{port}/"), hits)
    }

    #[test]
    fn pictures_from_the_internet_load_only_with_permission() {
        let Some(printer) = printer() else { return };
        let (base, hits) = counting_server();
        let dir = tempfile::tempdir().unwrap();
        let source = page_file(
            dir.path(),
            &format!("<p>Text</p><img src=\"{base}logo.png\">"),
        );
        let dest = dir.path().join("o.pdf");
        print_file(&printer, &source, &HtmlToPdfOptions::default(), &dest).unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 0, "nothing left the computer");

        let allowed = HtmlToPdfOptions {
            allow_network: true,
            ..HtmlToPdfOptions::default()
        };
        print_file(&printer, &source, &allowed, &dest).unwrap();
        assert!(hits.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn a_web_address_prints_after_consent() {
        let Some(printer) = printer() else { return };
        let (base, hits) = counting_server();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("web.pdf");
        let options = HtmlToPdfOptions {
            url: Some(base.clone()),
            allow_network: true,
            ..HtmlToPdfOptions::default()
        };
        printer
            .print(
                &HtmlSource::Url(base),
                &options,
                &dest,
                &CancellationToken::new(),
            )
            .unwrap();
        assert!(hits.load(Ordering::SeqCst) >= 1);
        assert_eq!(pages(&dest).len(), 1);
    }

    #[test]
    fn an_address_that_cannot_be_reached_is_a_network_error() {
        let Some(printer) = printer() else { return };
        let dir = tempfile::tempdir().unwrap();
        let options = HtmlToPdfOptions {
            allow_network: true,
            ..HtmlToPdfOptions::default()
        };
        let err = printer
            .print(
                &HtmlSource::Url("http://nonexistent.invalid/".to_owned()),
                &options,
                &dir.path().join("x.pdf"),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable, "{}", err.message);
    }

    #[test]
    fn cancelling_stops_the_browser_and_leaves_nothing_behind() {
        let Some(printer) = printer() else { return };
        let dir = tempfile::tempdir().unwrap();
        let source = page_file(dir.path(), "<p>x</p>");
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = printer
            .print(
                &HtmlSource::File(source),
                &HtmlToPdfOptions::default(),
                &dir.path().join("x.pdf"),
                &cancel,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::JobCancelled);
        assert!(!dir.path().join("x.pdf").exists());
    }

    #[test]
    fn a_missing_browser_is_a_missing_component() {
        let printer = EdgePrinter::with_program(PathBuf::from("Z:\\nowhere\\msedge.exe"));
        assert!(!printer.is_available());
        let dir = tempfile::tempdir().unwrap();
        let err = print_file(
            &printer,
            &dir.path().join("a.html"),
            &HtmlToPdfOptions::default(),
            &dir.path().join("o.pdf"),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }

    #[test]
    fn local_files_get_addresses_a_browser_understands() {
        assert_eq!(
            file_address(Path::new("C:\\a b\\p#1.html")),
            "file:///C:/a%20b/p%231.html"
        );
    }

    #[test]
    fn print_parameters_carry_paper_margins_and_scale() {
        let options = HtmlToPdfOptions {
            scale_percent: 80,
            margin_mm: 25.4,
            header_footer: true,
            ..HtmlToPdfOptions::default()
        };
        let p = print_parameters(&options).unwrap();
        assert_eq!(p["scale"], 0.8);
        assert_eq!(p["marginTop"], 1.0);
        assert_eq!(p["displayHeaderFooter"], true);
        assert!((p["paperWidth"].as_f64().unwrap() - 8.268).abs() < 0.01);
    }
}
