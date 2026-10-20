//! Shared harness for the end-to-end tests: the real `RunTool`, the worker engines (PDFium, qpdf,
//! lopdf, raster) and the file store, run on generated PDFs in a temporary folder.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use application::ports::{OcrEngine, PdfReader, Renderer, TextExtractor};
use application::{
    AppError, CancellationToken, InputDoc, NoProgress, OutputArtifact, Password, UnitRequest,
    UnitRunner,
};
use domain::naming::OutputNamingPattern;
use domain::tools::common::Region;
use domain::{Dpi, PageIndex, ToolOptions};
use infrastructure::{AtomicFileStore, NativePaths, SystemClock, TracingLogger};
use quirewise_engine_pdfium::PdfiumEngine;
use quirewise_worker::engine::{Engines, build_services, tool_deps};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use test_support::{pdfium_dir, qpdf_exe};

pub struct Lab {
    pub dir: tempfile::TempDir,
    pub runner: Arc<dyn UnitRunner>,
    pub queries: Arc<dyn application::QueryRunner>,
    pub engine: PdfiumEngine,
    pub ocr: Arc<dyn OcrEngine>,
}

/// What a rendered page looks like in numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Pixels that are not paper white.
    pub inked: usize,
    /// Pixels whose color channels differ clearly, so not gray.
    pub colored: usize,
    /// Average horizontal position of the inked pixels, 0 (left) to 1 (right).
    pub center_x: f32,
    /// Average vertical position of the inked pixels, 0 (top) to 1 (bottom).
    pub center_y: f32,
}

impl Lab {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = NativePaths {
            pdfium_dir: pdfium_dir(),
            qpdf_exe: qpdf_exe().unwrap(),
        };
        let engines = Engines::load(&paths).unwrap();
        let services = build_services(
            &engines,
            Arc::new(AtomicFileStore),
            Arc::new(SystemClock),
            Arc::new(TracingLogger),
            Default::default(),
            dir.path().join("scratch"),
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("out")).unwrap();
        Self {
            dir,
            runner: services.runner,
            queries: services.queries,
            ocr: tool_deps(&engines).ocr,
            engine: engines.pdfium,
        }
    }

    /// True when Tesseract and the language are installed, so OCR tests can run.
    pub fn ocr_ready(&self, language: &str) -> bool {
        self.ocr.is_available()
            && self
                .ocr
                .languages()
                .is_ok_and(|l| l.iter().any(|x| x == language))
    }

    pub fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    pub fn doc(&self, name: &str, bytes: &[u8]) -> InputDoc {
        InputDoc::new(self.put(name, bytes))
    }

    pub fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    pub fn run_all(
        &self,
        options: ToolOptions,
        inputs: Vec<InputDoc>,
    ) -> Result<Vec<OutputArtifact>, AppError> {
        let request = UnitRequest {
            tool: options.tool_id(),
            options,
            inputs,
            output_dir: Some(self.out()),
            naming_pattern: OutputNamingPattern::default(),
        };
        self.runner
            .run_unit(&request, &CancellationToken::new(), &NoProgress)
    }

    pub fn run(
        &self,
        options: ToolOptions,
        input: InputDoc,
    ) -> Result<Vec<OutputArtifact>, AppError> {
        self.run_all(options, vec![input])
    }

    /// The text of every page.
    pub fn texts(&self, path: &Path, password: Option<&Password>) -> Vec<String> {
        let count = self.engine.open_info(path, password).unwrap().page_count;
        (0..count)
            .map(|i| {
                self.engine
                    .page_text(path, password, PageIndex::new(i))
                    .unwrap()
                    .trim()
                    .to_owned()
            })
            .collect()
    }

    /// The text inside a region of the displayed page.
    pub fn text_in(&self, path: &Path, page: u32, region: Region) -> String {
        self.engine
            .text_in_region(path, None, PageIndex::new(page), &region)
            .unwrap()
    }

    /// Page sizes as displayed.
    pub fn sizes(&self, path: &Path) -> Vec<(f32, f32)> {
        self.engine
            .page_sizes(path, None)
            .unwrap()
            .iter()
            .map(|s| (s.width_pt, s.height_pt))
            .collect()
    }

    pub fn look(&self, path: &Path, page: u32, password: Option<&Password>) -> Look {
        let image = self
            .engine
            .render_page(path, password, PageIndex::new(page), Dpi::new(50).unwrap())
            .unwrap();
        let (w, h) = (image.width as usize, image.height as usize);
        let (mut inked, mut colored, mut sum_x, mut sum_y) = (0usize, 0usize, 0.0f64, 0.0f64);
        for (i, px) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
            let max = px[0].max(px[1]).max(px[2]);
            let min = px[0].min(px[1]).min(px[2]);
            if min < 235 {
                inked += 1;
                sum_x += (i % w) as f64;
                sum_y += (i / w) as f64;
            }
            if max - min > 40 {
                colored += 1;
            }
        }
        let n = inked.max(1) as f64;
        Look {
            inked,
            colored,
            center_x: (sum_x / n / w as f64) as f32,
            center_y: (sum_y / n / h as f64) as f32,
        }
    }

    /// What no run may leave behind: scratch files or temporary outputs.
    pub fn tidy(&self) {
        let scratch = self.dir.path().join("scratch");
        assert_eq!(
            fs::read_dir(&scratch).unwrap().count(),
            0,
            "scratch files remain"
        );
        for entry in fs::read_dir(self.out()).unwrap().flatten() {
            assert!(!entry.file_name().to_string_lossy().ends_with(".qwtmp"));
        }
    }
}

pub fn name(output: &OutputArtifact) -> String {
    output
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}
