//! Engine wiring for the worker: loads the native engines and composes them into the ports the
//! use cases need. Engines never import each other, so any combination happens here.

use application::inspect::InspectDocument;
use application::ports::{
    Clock, ComponentStore, FileStore, Logger, OfficeEngines, PdfReader, PdfVerifier,
};
use application::query::{AiPorts, EditorPorts, QueryService};
use application::tools::{ToolDeps, register_all};
use application::{AppError, Password, RunTool, ToolRegistry};
use domain::ErrorCode;
use domain::limits::SafetyLimits;
use infrastructure::{AppPaths, FsComponentStore, HttpWeb, NativePaths, native::arch_dir};
use quirewise_engine_ai_llamacpp::LlamaCpp;
use quirewise_engine_export::OfficeWriter;
use quirewise_engine_html_webview2::EdgePrinter;
use quirewise_engine_lopdf_ops::{
    InstalledFonts, LopdfAttachments, LopdfEditor, LopdfForms, LopdfImages, LopdfPdfa,
    LopdfStructure, LopdfTextDoc, LopdfToc,
};
use quirewise_engine_ocr_tesseract::Tesseract;
use quirewise_engine_office_libreoffice::LibreOffice;
use quirewise_engine_office_msoffice::MsOffice;
use quirewise_engine_pdfa_verapdf::VeraPdf;
use quirewise_engine_pdfium::PdfiumEngine;
use quirewise_engine_qpdf::QpdfCli;
use quirewise_engine_raster::RasterConverter;
use quirewise_engine_sign_cms::CmsEngine;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Services;
use crate::job_object::SidecarSpawner;

/// The loaded native engines.
#[derive(Clone)]
pub struct Engines {
    /// PDFium: rendering, text, facts.
    pub pdfium: PdfiumEngine,
    /// qpdf sidecar: checks and page copying.
    pub qpdf: QpdfCli,
    /// Components installed on demand, such as LibreOffice.
    pub components: Arc<dyn ComponentStore>,
    /// The language model on this computer. One for the worker, so the server it starts is shared
    /// by the tools and the queries, and stops when the worker does.
    pub llm: Arc<LlamaCpp>,
}

impl Engines {
    /// Loads PDFium and points at the qpdf executable.
    pub fn load(paths: &NativePaths) -> Result<Self, AppError> {
        let components: Arc<dyn ComponentStore> = Arc::new(FsComponentStore::new(
            AppPaths::from_environment()
                .map(|p| p.components_dir)
                .unwrap_or_default(),
        ));
        let (server_dirs, model_dirs) = llama_folders();
        Ok(Self {
            pdfium: PdfiumEngine::load(&paths.pdfium_dir)?,
            qpdf: QpdfCli::new(paths.qpdf_exe.clone()),
            llm: Arc::new(LlamaCpp::with_spawner(
                Some(Arc::clone(&components)),
                server_dirs,
                model_dirs,
                Arc::new(SidecarSpawner),
            )),
            components,
        })
    }
}

/// Verifies written PDFs: reopen in PDFium, check the page count, then run `qpdf --check`.
pub struct CompositeVerifier {
    engines: Engines,
}

impl CompositeVerifier {
    /// Creates a verifier over the loaded engines.
    pub fn new(engines: Engines) -> Self {
        Self { engines }
    }
}

fn failed(why: &str) -> AppError {
    AppError::new(
        ErrorCode::OutputVerifyFailed,
        format!(
            "The result did not pass verification ({why}). Your original file was not changed."
        ),
    )
}

impl PdfVerifier for CompositeVerifier {
    fn verify(
        &self,
        path: &Path,
        password: Option<&Password>,
        expected_pages: Option<u32>,
    ) -> Result<(), AppError> {
        let info = self
            .engines
            .pdfium
            .open_info(path, password)
            .map_err(|_| failed("it could not be reopened"))?;
        if let Some(expected) = expected_pages
            && info.page_count != expected
        {
            return Err(failed("the page count is wrong"));
        }
        self.engines
            .qpdf
            .check(path, password)
            .map(|_| ())
            .map_err(|_| failed("the structure check failed"))
    }
}

/// Folders next to the app that may hold Tesseract: the installed layout and a development tree.
fn tesseract_folders() -> Vec<PathBuf> {
    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let mut folders = Vec::new();
    for root in exe.ancestors().skip(1).take(7) {
        folders.push(root.join("native").join("tesseract"));
        folders.push(root.join("vendor").join("tesseract").join(arch_dir()));
    }
    folders
}

/// Folders next to the app that may hold veraPDF: the installed layout and a development tree.
fn verapdf_folders() -> Vec<PathBuf> {
    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let mut folders = Vec::new();
    for root in exe.ancestors().skip(1).take(7) {
        folders.push(root.join("native").join("verapdf"));
        folders.push(root.join("vendor").join("verapdf"));
    }
    folders
}

/// Folders next to the app that may hold the llama.cpp server and model files: the installed
/// layout and a development tree.
fn llama_folders() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Ok(exe) = std::env::current_exe() else {
        return (Vec::new(), Vec::new());
    };
    let (mut servers, mut models) = (Vec::new(), Vec::new());
    for root in exe.ancestors().skip(1).take(7) {
        servers.push(root.join("native").join("llama-cpp"));
        servers.push(root.join("vendor").join("llama-cpp").join(arch_dir()));
        models.push(root.join("vendor").join("models"));
    }
    (servers, models)
}

/// The Ghostscript plugin, in a build that has it and on a computer that has Ghostscript.
#[cfg(feature = "agpl-ghostscript")]
fn ghostscript() -> Option<Arc<dyn application::ports::PdfaNormalizer>> {
    let mut folders = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432"] {
        if let Some(dir) = std::env::var_os(var) {
            folders.push(PathBuf::from(dir).join("gs"));
        }
    }
    let gs = quirewise_engine_gs_plugin::Ghostscript::new(None, folders);
    gs.is_available()
        .then(|| Arc::new(gs) as Arc<dyn application::ports::PdfaNormalizer>)
}

/// Without the plugin there is no second way.
#[cfg(not(feature = "agpl-ghostscript"))]
fn ghostscript() -> Option<Arc<dyn application::ports::PdfaNormalizer>> {
    None
}

/// The ports every tool is built on, backed by the loaded engines.
pub fn tool_deps(engines: &Engines) -> ToolDeps {
    ToolDeps {
        reader: Arc::new(engines.pdfium),
        writer: Arc::new(engines.qpdf.clone()),
        editor: Arc::new(LopdfEditor),
        toc: Arc::new(LopdfToc),
        images: Arc::new(RasterConverter),
        outlines: Arc::new(engines.pdfium),
        text: Arc::new(engines.pdfium),
        flattener: Arc::new(engines.pdfium),
        verifier: Arc::new(CompositeVerifier::new(engines.clone())),
        renderer: Arc::new(engines.pdfium),
        security: Arc::new(engines.qpdf.clone()),
        packager: Arc::new(engines.qpdf.clone()),
        // qpdf first: it changes the least. PDFium rewrites everything it manages to load.
        repairers: vec![Arc::new(engines.qpdf.clone()), Arc::new(engines.pdfium)],
        optimizer: Arc::new(RasterConverter),
        page_images: Arc::new(RasterConverter),
        attachments: Arc::new(LopdfAttachments),
        headings: Arc::new(engines.pdfium),
        pictures: Arc::new(RasterConverter),
        extractor: Arc::new(LopdfImages),
        office: OfficeEngines {
            microsoft: Arc::new(MsOffice::new()),
            libre: Arc::new(LibreOffice::new(Some(Arc::clone(&engines.components)))),
        },
        html: Arc::new(EdgePrinter::new()),
        ocr: Arc::new(Tesseract::new(
            Some(Arc::clone(&engines.components)),
            tesseract_folders(),
        )),
        skew: Arc::new(RasterConverter),
        scans: Arc::new(RasterConverter),
        layout: Arc::new(engines.pdfium),
        docs: Arc::new(OfficeWriter),
        signer: Arc::new(CmsEngine::new()),
        signatures: Arc::new(CmsEngine::new()),
        web: Arc::new(HttpWeb::new()),
        normalizer: Arc::new(LopdfPdfa),
        pdfa_fallback: ghostscript(),
        llm: engines.llm.clone(),
        textdoc: Arc::new(LopdfTextDoc),
        conformance: Arc::new(VeraPdf::new(
            Some(Arc::clone(&engines.components)),
            verapdf_folders(),
        )),
    }
}

/// Everything the worker serves with: every tool registered over the loaded engines, plus the
/// read-only queries. Creates the scratch folder.
pub fn build_services(
    engines: &Engines,
    files: Arc<dyn FileStore>,
    clock: Arc<dyn Clock>,
    logger: Arc<dyn Logger>,
    limits: SafetyLimits,
    temp_dir: PathBuf,
) -> Result<Services, AppError> {
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| AppError::from_io("create the scratch folder", &e))?;
    let mut registry = ToolRegistry::new();
    let deps = tool_deps(engines);
    register_all(&mut registry, &deps)?;
    let reader: Arc<dyn PdfReader> = Arc::new(engines.pdfium);
    let inspect = InspectDocument::new(
        Arc::clone(&files),
        Arc::new(engines.pdfium),
        Arc::new(LopdfStructure),
        limits,
    );
    let queries = QueryService::new(
        Arc::clone(&files),
        Arc::clone(&reader),
        Arc::new(engines.pdfium),
        deps.ocr.clone(),
        inspect,
        limits,
    )
    .with_editor(EditorPorts {
        layout: Arc::new(engines.pdfium),
        forms: Arc::new(LopdfForms),
        fonts: Arc::new(InstalledFonts),
    })
    .with_signer(Arc::new(CmsEngine::new()))
    .with_ai(AiPorts {
        text: Arc::new(engines.pdfium),
        llm: engines.llm.clone(),
    });
    let runner = RunTool::new(
        Arc::new(registry),
        reader,
        files,
        clock,
        logger,
        limits,
        temp_dir,
    );
    Ok(Services {
        runner: Arc::new(runner),
        queries: Arc::new(queries),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::query::{DocumentQuery, QueryResult};
    use application::{CancellationToken, DocumentReport};
    use domain::ErrorCode;
    use infrastructure::{AtomicFileStore, SystemClock, TracingLogger};
    use test_support::{encrypt_pdf, minimal_pdf, pdfium_dir, qpdf_exe};

    fn engines() -> Engines {
        let paths = NativePaths {
            pdfium_dir: pdfium_dir(),
            qpdf_exe: qpdf_exe().expect("run scripts/fetch-qpdf.ps1 first"),
        };
        Engines::load(&paths).expect("run scripts/fetch-pdfium.ps1 first")
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn verifier_accepts_good_output_and_rejects_wrong_counts_and_damage() {
        let dir = tempfile::tempdir().unwrap();
        let good = write(&dir, "a.pdf", &minimal_pdf(3));
        let v = CompositeVerifier::new(engines());
        assert!(v.verify(&good, None, Some(3)).is_ok());
        assert!(v.verify(&good, None, None).is_ok());
        assert_eq!(
            v.verify(&good, None, Some(4)).unwrap_err().code,
            ErrorCode::OutputVerifyFailed
        );
        let bad = write(&dir, "b.pdf", b"%PDF-1.7 truncated");
        assert_eq!(
            v.verify(&bad, None, None).unwrap_err().code,
            ErrorCode::OutputVerifyFailed
        );
    }

    fn services() -> Services {
        build_services(
            &engines(),
            Arc::new(AtomicFileStore),
            Arc::new(SystemClock),
            Arc::new(TracingLogger),
            SafetyLimits::default(),
            std::env::temp_dir().join("quirewise-tests"),
        )
        .unwrap()
    }

    fn inspect(path: &Path, password: Option<Password>) -> Result<DocumentReport, AppError> {
        let q = DocumentQuery::Inspect {
            path: path.to_path_buf(),
            password,
        };
        match services().queries.query(&q, &CancellationToken::new())? {
            QueryResult::Inspect(r) => Ok(*r),
            other => panic!("unexpected result {other:?}"),
        }
    }

    #[test]
    fn inspect_end_to_end_on_a_plain_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.pdf", &minimal_pdf(4));
        let r = inspect(&path, None).unwrap();
        assert!(!r.locked);
        assert_eq!((r.page_count, r.pdf_version.as_deref()), (4, Some("1.7")));
        assert!(r.has_text_layer && r.encryption.is_none() && !r.has_javascript);
        assert!(r.suggestions.is_empty());
        assert!(r.file_size > 0 && r.size_breakdown.content > 0);
    }

    #[test]
    fn inspect_end_to_end_on_a_locked_pdf_suggests_unlock() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write(&dir, "a.pdf", &minimal_pdf(2));
        let locked = dir.path().join("locked.pdf");
        encrypt_pdf(&plain, &locked, "open");

        let r = inspect(&locked, None).unwrap();
        assert!(r.locked);
        assert_eq!(
            r.suggestions,
            vec![application::inspect::Suggestion::Unlock]
        );
        assert_eq!(
            r.encryption.as_ref().map(|e| e.method.as_str()),
            Some("AES-256")
        );

        let unlocked = inspect(&locked, Some(Password::new("open"))).unwrap();
        assert!(!unlocked.locked);
        assert_eq!(unlocked.page_count, 2);

        let wrong = inspect(&locked, Some(Password::new("nope"))).unwrap_err();
        assert_eq!(wrong.code, ErrorCode::PdfWrongPassword);
    }

    #[test]
    fn thumbnails_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.pdf", &minimal_pdf(2));
        let q = DocumentQuery::Thumbnail {
            path,
            password: None,
            page: 1,
            max_edge: 160,
        };
        let QueryResult::Thumbnail(t) = services()
            .queries
            .query(&q, &CancellationToken::new())
            .unwrap()
        else {
            panic!("expected a thumbnail");
        };
        assert_eq!((t.page_count, t.height), (2, 160));
        assert_eq!(&t.png.0[1..4], b"PNG");
    }

    #[test]
    fn non_pdf_files_are_rejected_before_any_parser_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "x.pdf", b"PK\x03\x04 zip data");
        assert_eq!(
            inspect(&path, None).unwrap_err().code,
            ErrorCode::InputNotPdf
        );
    }
}
