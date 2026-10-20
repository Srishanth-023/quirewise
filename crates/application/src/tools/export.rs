//! The tools that turn a PDF into an editable document: PDF to Word (row 25), PDF to PowerPoint
//! (row 26), PDF to Excel (row 27), PDF to Markdown (row 29) and PDF to HTML (row 67).
//!
//! All five read the pages the same way: PDFium gives the words, lines and pictures of a page,
//! pages that are scans are read with OCR, and the pure layout analysis in `domain::layout`
//! decides what is a heading, a paragraph, a list item or a table.

use std::collections::HashMap;

use domain::layout::render::{self, base64};
use domain::layout::{Block, Detection, PageContent, PageLayout, PictureBox, Rect, Table, Word};
use domain::tools::ToolGroup;
use domain::tools::export::{
    EXPORT_OCR_DPI, ExportOcr, PdfToExcelOptions, PdfToHtmlOptions, PdfToMarkdownOptions,
    PdfToPowerPointOptions, PdfToWordOptions, SheetFormat, SheetGrouping, SlideMode, WordLayout,
};
use domain::{Dpi, Duplicates, ErrorCode, PageIndex, ToolId, ToolOptions};

use super::common::{Progress, ToolDeps, first_input, page_count, publish_file, wrong_options};
use super::recognize::check_ready;
use crate::error::AppError;
use crate::ports::{OcrWord, PictureBytes, SheetData};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Alphanumeric characters that make a page count as having text.
const TEXT_THRESHOLD: usize = 10;
/// A picture this much of the page is a scan of it.
const SCAN_COVERAGE: f32 = 0.5;
/// A picture this much of a page that has text is the scan that text was read from.
const BACKGROUND_COVERAGE: f32 = 0.8;

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions, units: u64) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: units,
    }
}

fn selected_pages(range: &domain::PageRange, count: u32) -> Result<Vec<PageIndex>, AppError> {
    let mut pages: Vec<PageIndex> = range.resolve(count, Duplicates::Reject)?.pages().to_vec();
    pages.sort_by_key(|p| p.get());
    Ok(pages)
}

// ----- reading pages -----------------------------------------------------------------------

fn alnum_count(content: &PageContent) -> usize {
    content
        .words
        .iter()
        .flat_map(|w| w.text.chars())
        .filter(|c| c.is_alphanumeric())
        .count()
}

fn coverage(content: &PageContent, rect: &Rect) -> f32 {
    let page = (content.width * content.height).max(1.0);
    rect.width() * rect.height() / page
}

fn covers_page(content: &PageContent, rect: &Rect) -> bool {
    coverage(content, rect) >= SCAN_COVERAGE
}

/// A page that was scanned and then read has the scan under its text. The text is what is
/// converted, so a picture that nearly fills such a page is left out instead of repeating it.
fn without_scan_background(mut content: PageContent) -> PageContent {
    if alnum_count(&content) >= TEXT_THRESHOLD {
        let snapshot = content.clone();
        content
            .pictures
            .retain(|p| coverage(&snapshot, &p.rect) < BACKGROUND_COVERAGE);
    }
    content
}

/// True when one picture fills at least half the page, as a scan does.
fn looks_scanned(content: &PageContent) -> bool {
    content
        .pictures
        .iter()
        .any(|p| covers_page(content, &p.rect))
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values
        .get(values.len().saturating_sub(1) / 2)
        .copied()
        .unwrap_or(0.0)
}

/// Replaces the words of a page with the words OCR read from its picture, and drops the picture
/// of the scan itself. Line heights are made even (a line without tall letters is not smaller
/// text) and each line gets a box of one font size, so paragraphs join as they do for real text.
fn with_ocr_words(mut content: PageContent, words: &[OcrWord], image: (u32, u32)) -> PageContent {
    let sx = content.width / image.0.max(1) as f32;
    let sy = content.height / image.1.max(1) as f32;

    // One entry per line: tallest word, and the middle of its words.
    let mut lines: Vec<(u32, f32, f32, usize)> = Vec::new();
    for w in words {
        let mid = (w.top + w.height / 2.0) * sy;
        let tall = w.height * sy;
        match lines.iter_mut().find(|l| l.0 == w.line) {
            Some(l) => {
                l.1 = l.1.max(tall);
                l.2 += mid;
                l.3 += 1;
            }
            None => lines.push((w.line, tall, mid, 1)),
        }
    }
    let mut heights: Vec<f32> = lines.iter().map(|l| l.1).collect();
    let typical = median(&mut heights);
    let size_of = |tall: f32| {
        let size = if typical > 0.0 && (tall - typical).abs() <= typical * 0.3 {
            typical
        } else {
            tall
        };
        size.max(4.0)
    };

    content.words = words
        .iter()
        .filter_map(|w| {
            let (_, tall, mid_sum, n) = *lines.iter().find(|l| l.0 == w.line)?;
            let size = size_of(tall);
            let mid = mid_sum / n as f32;
            Some(Word {
                text: w.text.clone(),
                rect: Rect::new(
                    w.left * sx,
                    mid - size / 2.0,
                    (w.left + w.width) * sx,
                    mid + size / 2.0,
                ),
                size,
                bold: false,
                italic: false,
            })
        })
        .collect();
    let snapshot = content.clone();
    content
        .pictures
        .retain(|p| !covers_page(&snapshot, &p.rect));
    content
}

/// Reads the content of the pages, with OCR for pages that need it.
fn read_pages(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    pages: &[PageIndex],
    ocr: ExportOcr,
    languages: &[String],
    progress: &mut Progress<'_>,
) -> Result<Vec<PageContent>, AppError> {
    let mut ready = false;
    let mut out = Vec::with_capacity(pages.len());
    for page in pages {
        ctx.cancel.check()?;
        let content = deps
            .layout
            .page_content(&input.path, input.password.as_ref(), *page)?;
        let wants_ocr = match ocr {
            ExportOcr::Off => false,
            ExportOcr::Always => true,
            ExportOcr::Auto => alnum_count(&content) < TEXT_THRESHOLD && looks_scanned(&content),
        };
        if !wants_ocr {
            out.push(without_scan_background(content));
            progress.advance(1);
            continue;
        }
        if !ready {
            check_ready(deps, languages)?;
            ready = true;
        }
        let dpi = Dpi::new(EXPORT_OCR_DPI)?;
        let image = deps
            .renderer
            .render_page(&input.path, input.password.as_ref(), *page, dpi)?;
        ctx.cancel.check()?;
        let words = deps
            .ocr
            .words(&image, languages, dpi, ctx.temp_dir, ctx.cancel)?;
        out.push(with_ocr_words(content, &words, (image.width, image.height)));
        progress.advance(1);
    }
    Ok(out)
}

/// How the pages of a run are read.
struct Reading<'a> {
    ocr: ExportOcr,
    languages: &'a [String],
    detection: Detection,
}

/// Reads and analyzes the pages.
fn analyzed(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    pages: &[PageIndex],
    reading: &Reading<'_>,
    progress: &mut Progress<'_>,
) -> Result<Vec<PageLayout>, AppError> {
    let contents = read_pages(
        ctx,
        deps,
        input,
        pages,
        reading.ocr,
        reading.languages,
        progress,
    )?;
    Ok(domain::layout::analyze(contents, reading.detection))
}

/// Where a writer gets the pictures of the input.
fn picture_source<'a>(
    ctx: &'a JobContext<'a>,
    deps: &'a ToolDeps,
    input: &'a InputDoc,
) -> impl FnMut(u32, usize) -> Result<PictureBytes, AppError> + 'a {
    move |page, index| {
        ctx.cancel.check()?;
        deps.layout.picture(
            &input.path,
            input.password.as_ref(),
            PageIndex::new(page.saturating_sub(1)),
            index,
        )
    }
}

/// Writes bytes as a new file.
fn write_bytes(
    ctx: &JobContext<'_>,
    dest: &std::path::Path,
    bytes: &[u8],
) -> Result<OutputArtifact, AppError> {
    ctx.cancel.check()?;
    let path = ctx
        .files
        .write_atomic(dest, &mut |w| w.write_all(bytes), &|_| Ok(()))?;
    Ok(OutputArtifact {
        path,
        pages: None,
        bytes: bytes.len() as u64,
    })
}

fn extension_of(picture: &PictureBytes) -> &'static str {
    if picture.jpeg { "jpg" } else { "png" }
}

/// The file names of saved pictures by page and picture, and the files as outputs.
type SavedPictures = (HashMap<(u32, usize), String>, Vec<OutputArtifact>);

/// Saves every picture of the layouts as a file next to the output and returns the file names
/// by page and picture, with the files as outputs.
fn save_pictures(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    layouts: &[PageLayout],
) -> Result<SavedPictures, AppError> {
    let mut names = HashMap::new();
    let mut outputs = Vec::new();
    let mut source = picture_source(ctx, deps, input);
    let mut n = 0u32;
    for page in layouts {
        for block in &page.blocks {
            let Block::Picture(p) = block else { continue };
            let picture = match source(page.number, p.index) {
                Ok(p) => p,
                Err(e) if e.code == ErrorCode::JobCancelled => return Err(e),
                Err(_) => continue,
            };
            n += 1;
            let dest = ctx.output_path(
                input,
                &format!("image {n}"),
                extension_of(&picture),
                |name| name.page = Some(page.number),
            )?;
            let out = write_bytes(ctx, &dest, &picture.bytes)?;
            if let Some(file) = out.path.file_name() {
                names.insert((page.number, p.index), file.to_string_lossy().into_owned());
            }
            outputs.push(out);
        }
    }
    Ok((names, outputs))
}

/// A file name as a Markdown or HTML link target: spaces and brackets are percent-encoded.
fn link_target(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            c => out.push(c),
        }
    }
    out
}

// ----- PDF to Word -------------------------------------------------------------------------

/// Writes a PDF as a Word document.
pub struct PdfToWordTool {
    deps: ToolDeps,
}

impl PdfToWordTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for PdfToWordTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToWord
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "word"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToWordOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::PdfToWord(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        Ok(plan_for(
            ToolId::PdfToWord,
            inputs,
            opts,
            pages.len() as u64 + 1,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::PdfToWord(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let layouts = analyzed(
            ctx,
            &self.deps,
            input,
            &pages,
            &Reading {
                ocr: options.ocr,
                languages: &options.languages,
                detection: Detection::Auto,
            },
            &mut progress,
        )?;
        let dest = ctx.output_path(input, self.label(), "docx", |_| {})?;
        let title = input.stem();
        let mut source = picture_source(ctx, &self.deps, input);
        let out = publish_file(
            ctx,
            &dest,
            &mut |tmp| {
                self.deps.docs.write_docx(
                    &layouts,
                    &title,
                    options.layout == WordLayout::Exact,
                    &mut source,
                    tmp,
                )
            },
            &|tmp| self.deps.docs.verify(tmp, 0),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

// ----- PDF to PowerPoint -------------------------------------------------------------------

/// Writes a PDF as a presentation, one slide per page.
pub struct PdfToPowerPointTool {
    deps: ToolDeps,
}

impl PdfToPowerPointTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for PdfToPowerPointTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToPowerpoint
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "powerpoint"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToPowerPointOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::PdfToPowerpoint(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        Ok(plan_for(
            ToolId::PdfToPowerpoint,
            inputs,
            opts,
            pages.len() as u64 + 1,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::PdfToPowerpoint(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let title = input.stem();
        let dest = ctx.output_path(input, self.label(), "pptx", |_| {})?;
        let slides = pages.len() as u32;
        let verify = |tmp: &std::path::Path| self.deps.docs.verify(tmp, slides);

        let out = match options.mode {
            SlideMode::EditableText => {
                let layouts = analyzed(
                    ctx,
                    &self.deps,
                    input,
                    &pages,
                    &Reading {
                        ocr: options.ocr,
                        languages: &options.languages,
                        detection: Detection::Auto,
                    },
                    &mut progress,
                )?;
                let mut source = picture_source(ctx, &self.deps, input);
                publish_file(
                    ctx,
                    &dest,
                    &mut |tmp| {
                        self.deps
                            .docs
                            .write_pptx(&layouts, &title, &mut source, tmp)
                    },
                    &verify,
                )?
            }
            SlideMode::ImageSlides => {
                let all = self
                    .deps
                    .reader
                    .page_sizes(&input.path, input.password.as_ref())?;
                let sizes: Vec<(f32, f32)> = pages
                    .iter()
                    .map(|p| {
                        all.get(p.get() as usize)
                            .map_or((720.0, 540.0), |s| (s.width_pt, s.height_pt))
                    })
                    .collect();
                let mut render = |i: usize| {
                    ctx.cancel.check()?;
                    let image = self.deps.renderer.render_page(
                        &input.path,
                        input.password.as_ref(),
                        pages[i],
                        options.dpi,
                    )?;
                    progress.advance(1);
                    Ok(image)
                };
                publish_file(
                    ctx,
                    &dest,
                    &mut |tmp| {
                        self.deps
                            .docs
                            .write_image_pptx(&sizes, &title, &mut render, tmp)
                    },
                    &verify,
                )?
            }
        };
        Ok(vec![out])
    }
}

// ----- PDF to Excel ------------------------------------------------------------------------

/// A table found on a page.
struct Found<'a> {
    page: u32,
    /// Position among the tables of the page, from 1.
    number: usize,
    /// How many tables the page has.
    on_page: usize,
    table: &'a Table,
}

fn found_tables(layouts: &[PageLayout]) -> Vec<Found<'_>> {
    let mut out = Vec::new();
    for page in layouts {
        let tables: Vec<&Table> = page
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .collect();
        for (i, table) in tables.iter().enumerate() {
            out.push(Found {
                page: page.number,
                number: i + 1,
                on_page: tables.len(),
                table,
            });
        }
    }
    out
}

fn rows_of(table: &Table) -> Vec<Vec<String>> {
    table
        .rows
        .iter()
        .map(|r| r.iter().map(|c| c.text.clone()).collect())
        .collect()
}

/// One sheet per table, or one per page with its tables one under the other.
fn sheets_of(found: &[Found<'_>], grouping: SheetGrouping) -> Vec<SheetData> {
    match grouping {
        SheetGrouping::PerTable => found
            .iter()
            .map(|f| SheetData {
                name: if f.on_page > 1 {
                    format!("Page {} Table {}", f.page, f.number)
                } else {
                    format!("Page {}", f.page)
                },
                rows: rows_of(f.table),
            })
            .collect(),
        SheetGrouping::PerPage => {
            let mut sheets: Vec<SheetData> = Vec::new();
            for f in found {
                let name = format!("Page {}", f.page);
                match sheets.last_mut() {
                    Some(sheet) if sheet.name == name => {
                        sheet.rows.push(Vec::new());
                        sheet.rows.extend(rows_of(f.table));
                    }
                    _ => sheets.push(SheetData {
                        name,
                        rows: rows_of(f.table),
                    }),
                }
            }
            sheets
        }
    }
}

fn csv_of(rows: &[Vec<String>]) -> String {
    let mut out = String::from('\u{FEFF}');
    for row in rows {
        let fields: Vec<String> = row
            .iter()
            .map(|t| {
                if t.contains([',', '"', '\r', '\n']) {
                    format!("\"{}\"", t.replace('"', "\"\""))
                } else {
                    t.clone()
                }
            })
            .collect();
        out.push_str(&fields.join(","));
        out.push_str("\r\n");
    }
    out
}

/// Writes the tables of a PDF as a workbook or as CSV files.
pub struct PdfToExcelTool {
    deps: ToolDeps,
}

impl PdfToExcelTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for PdfToExcelTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToExcel
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "excel"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToExcelOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::PdfToExcel(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        Ok(plan_for(
            ToolId::PdfToExcel,
            inputs,
            opts,
            pages.len() as u64 + 1,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::PdfToExcel(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let layouts = analyzed(
            ctx,
            &self.deps,
            input,
            &pages,
            &Reading {
                ocr: options.ocr,
                languages: &options.languages,
                detection: options.detection,
            },
            &mut progress,
        )?;
        let found = found_tables(&layouts);
        if found.is_empty() {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "No tables were found on the pages you chose. Try another way of finding tables (lines or aligned text), or turn on text recognition for scanned pages.",
            ));
        }
        let sheets = sheets_of(&found, options.grouping);

        let outputs = match options.format {
            SheetFormat::Xlsx => {
                let dest = ctx.output_path(input, self.label(), "xlsx", |_| {})?;
                let count = sheets.len() as u32;
                vec![publish_file(
                    ctx,
                    &dest,
                    &mut |tmp| self.deps.docs.write_xlsx(&sheets, tmp),
                    &|tmp| self.deps.docs.verify(tmp, count),
                )?]
            }
            SheetFormat::Csv => {
                let mut outputs = Vec::with_capacity(sheets.len());
                for sheet in &sheets {
                    let first_page = sheet
                        .name
                        .split_whitespace()
                        .nth(1)
                        .and_then(|n| n.parse::<u32>().ok());
                    let dest =
                        ctx.output_path(input, &sheet.name.to_lowercase(), "csv", |name| {
                            name.page = first_page;
                        })?;
                    outputs.push(write_bytes(ctx, &dest, csv_of(&sheet.rows).as_bytes())?);
                }
                outputs
            }
        };
        progress.advance(1);
        Ok(outputs)
    }
}

// ----- PDF to Markdown and HTML ------------------------------------------------------------

/// Writes the text of a PDF as Markdown.
pub struct PdfToMarkdownTool {
    deps: ToolDeps,
}

impl PdfToMarkdownTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for PdfToMarkdownTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToMarkdown
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "markdown"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToMarkdownOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::PdfToMarkdown(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        Ok(plan_for(
            ToolId::PdfToMarkdown,
            inputs,
            opts,
            pages.len() as u64 + 1,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::PdfToMarkdown(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let layouts = analyzed(
            ctx,
            &self.deps,
            input,
            &pages,
            &Reading {
                ocr: options.ocr,
                languages: &options.languages,
                detection: Detection::Auto,
            },
            &mut progress,
        )?;
        let (names, mut outputs) = if options.images {
            save_pictures(ctx, &self.deps, input, &layouts)?
        } else {
            (HashMap::new(), Vec::new())
        };
        let text = render::markdown(&layouts, &|page: u32, p: &PictureBox| {
            names.get(&(page, p.index)).map(|n| link_target(n))
        });
        let dest = ctx.output_path(input, self.label(), "md", |_| {})?;
        outputs.insert(0, write_bytes(ctx, &dest, text.as_bytes())?);
        progress.advance(1);
        Ok(outputs)
    }
}

/// Writes a PDF as a web page.
pub struct PdfToHtmlTool {
    deps: ToolDeps,
}

impl PdfToHtmlTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for PdfToHtmlTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToHtml
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "html"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToHtmlOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::PdfToHtml(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        Ok(plan_for(
            ToolId::PdfToHtml,
            inputs,
            opts,
            pages.len() as u64 + 1,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::PdfToHtml(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let layouts = analyzed(
            ctx,
            &self.deps,
            input,
            &pages,
            &Reading {
                ocr: options.ocr,
                languages: &options.languages,
                detection: Detection::Auto,
            },
            &mut progress,
        )?;
        let title = input.stem();
        let (text, mut outputs) = if options.inline_images {
            let mut inline: HashMap<(u32, usize), String> = HashMap::new();
            let mut source = picture_source(ctx, &self.deps, input);
            for page in &layouts {
                for block in &page.blocks {
                    let Block::Picture(p) = block else { continue };
                    match source(page.number, p.index) {
                        Ok(pic) => {
                            let mime = if pic.jpeg { "image/jpeg" } else { "image/png" };
                            inline.insert(
                                (page.number, p.index),
                                format!("data:{mime};base64,{}", base64(&pic.bytes)),
                            );
                        }
                        Err(e) if e.code == ErrorCode::JobCancelled => return Err(e),
                        Err(_) => {}
                    }
                }
            }
            let text = render::html(&layouts, &title, &|page: u32, p: &PictureBox| {
                inline.get(&(page, p.index)).cloned()
            });
            (text, Vec::new())
        } else {
            let (names, files) = save_pictures(ctx, &self.deps, input, &layouts)?;
            let text = render::html(&layouts, &title, &|page: u32, p: &PictureBox| {
                names.get(&(page, p.index)).map(|n| link_target(n))
            });
            (text, files)
        };
        let dest = ctx.output_path(input, self.label(), "html", |_| {})?;
        outputs.insert(0, write_bytes(ctx, &dest, text.as_bytes())?);
        progress.advance(1);
        Ok(outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::PageRange;
    use domain::layout::{Cell, PictureBox};
    use domain::tools::export::ExportOcr;

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    fn word(text: &str, line: u32, left: f32, top: f32, height: f32) -> OcrWord {
        OcrWord {
            text: text.to_owned(),
            left,
            top,
            width: 20.0 * text.len() as f32,
            height,
            line,
            confidence: 90.0,
        }
    }

    fn blank(width: f32, height: f32) -> PageContent {
        PageContent {
            number: 1,
            width,
            height,
            words: vec![],
            rules: vec![],
            pictures: vec![],
        }
    }

    #[test]
    fn ocr_words_are_scaled_to_points_and_the_scan_picture_goes() {
        let mut page = blank(612.0, 792.0);
        page.pictures.push(PictureBox {
            rect: Rect::new(0.0, 0.0, 612.0, 792.0),
            index: 0,
        });
        page.pictures.push(PictureBox {
            rect: Rect::new(50.0, 50.0, 100.0, 100.0),
            index: 1,
        });
        // A 2550 by 3300 picture is a Letter page at 300 dpi.
        let words = [word("Hello", 0, 300.0, 300.0, 46.0)];
        let out = with_ocr_words(page, &words, (2550, 3300));
        assert_eq!(out.words.len(), 1);
        let w = &out.words[0];
        assert!((w.rect.x0 - 72.0).abs() < 0.01, "{w:?}");
        assert!((w.size - 11.04).abs() < 0.05, "{w:?}");
        // Only the small picture stays.
        assert_eq!(out.pictures.len(), 1);
        assert_eq!(out.pictures[0].index, 1);
    }

    #[test]
    fn ocr_lines_get_one_size_so_a_line_without_tall_letters_is_not_smaller_text() {
        let words = [
            word("Tall", 0, 100.0, 100.0, 46.0),
            word("flat", 1, 100.0, 160.0, 38.0),
            word("Tall", 2, 100.0, 220.0, 46.0),
            // A heading: much taller, keeps its own size.
            word("Title", 3, 100.0, 400.0, 120.0),
        ];
        let out = with_ocr_words(blank(612.0, 792.0), &words, (2550, 3300));
        let size_of = |t: &str, y: f32| {
            out.words
                .iter()
                .find(|w| w.text == t && (w.rect.y0 - y).abs() < 20.0)
                .map(|w| w.size)
                .unwrap()
        };
        let a = size_of("Tall", 100.0 * 0.24);
        let b = out.words[1].size;
        assert!((a - b).abs() < 0.01, "{a} vs {b}");
        assert!(out.words[3].size > a * 2.0);
    }

    #[test]
    fn ocr_lines_stack_close_enough_to_join_into_one_paragraph() {
        // Three lines 13 points apart, drawn from ink boxes of different heights.
        let words = [
            word("one", 0, 100.0, 100.0, 40.0),
            word("two", 1, 100.0, 154.0, 46.0),
            word("three", 2, 100.0, 208.0, 38.0),
        ];
        let out = with_ocr_words(blank(612.0, 792.0), &words, (2550, 3300));
        let layouts = domain::layout::analyze(vec![out], Detection::Auto);
        assert_eq!(layouts[0].blocks.len(), 1, "{:?}", layouts[0].blocks);
    }

    #[test]
    fn a_page_looks_scanned_only_when_a_picture_fills_it() {
        let mut page = blank(600.0, 800.0);
        assert!(!looks_scanned(&page));
        page.pictures.push(PictureBox {
            rect: Rect::new(10.0, 10.0, 200.0, 200.0),
            index: 0,
        });
        assert!(!looks_scanned(&page));
        page.pictures.push(PictureBox {
            rect: Rect::new(0.0, 0.0, 600.0, 700.0),
            index: 1,
        });
        assert!(looks_scanned(&page));
    }

    #[test]
    fn a_picture_under_real_text_is_a_scan_background_and_is_dropped() {
        let mut page = blank(600.0, 800.0);
        page.words = (0..4)
            .map(|i| Word {
                text: "wordsofthepage".to_owned(),
                rect: Rect::new(50.0, 50.0 + 14.0 * i as f32, 150.0, 62.0 + 14.0 * i as f32),
                size: 11.0,
                bold: false,
                italic: false,
            })
            .collect();
        page.pictures.push(PictureBox {
            rect: Rect::new(0.0, 0.0, 600.0, 800.0),
            index: 0,
        });
        page.pictures.push(PictureBox {
            rect: Rect::new(300.0, 300.0, 500.0, 500.0),
            index: 1,
        });
        let kept = without_scan_background(page.clone());
        assert_eq!(kept.pictures.len(), 1);
        assert_eq!(kept.pictures[0].index, 1);
        // Without text the picture is the content and stays.
        page.words.clear();
        assert_eq!(without_scan_background(page).pictures.len(), 2);
    }

    #[test]
    fn link_targets_are_percent_encoded() {
        assert_eq!(link_target("a b (1)#.png"), "a%20b%20%281%29%23.png");
    }

    #[test]
    fn sheets_are_named_by_page_and_table_and_grouped_on_request() {
        let cell = |t: &str| Cell {
            text: t.to_owned(),
            bold: false,
        };
        let table = |t: &str| Table {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            rows: vec![vec![cell(t)]],
            column_edges: vec![0.0],
            row_edges: vec![0.0],
        };
        let page = |number: u32, tables: &[&str]| PageLayout {
            number,
            width: 612.0,
            height: 792.0,
            blocks: tables.iter().map(|t| Block::Table(table(t))).collect(),
        };
        let layouts = [page(1, &["a"]), page(2, &["b", "c"]), page(3, &[])];
        let found = found_tables(&layouts);
        assert_eq!(found.len(), 3);
        let per_table = sheets_of(&found, SheetGrouping::PerTable);
        let names: Vec<&str> = per_table.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Page 1", "Page 2 Table 1", "Page 2 Table 2"]);
        let per_page = sheets_of(&found, SheetGrouping::PerPage);
        assert_eq!(per_page.len(), 2);
        assert_eq!(
            per_page[1].rows,
            vec![vec!["b".to_owned()], vec![], vec!["c".to_owned()]]
        );
    }

    #[test]
    fn csv_has_a_byte_order_mark_quotes_and_crlf() {
        let rows = vec![
            vec!["a,b".to_owned(), "say \"x\"".to_owned()],
            vec!["1".to_owned()],
        ];
        assert_eq!(csv_of(&rows), "\u{FEFF}\"a,b\",\"say \"\"x\"\"\"\r\n1\r\n");
    }

    #[test]
    fn the_tools_count_the_pages_plus_one_unit_for_writing() {
        let deps = ToolDeps::unavailable();
        let pages = PageRange::parse("2-4").unwrap();
        let word = ToolOptions::PdfToWord(PdfToWordOptions {
            pages: pages.clone(),
            ..PdfToWordOptions::default()
        });
        assert_eq!(
            PdfToWordTool::new(deps.clone())
                .validate(&[doc(6)], &word)
                .unwrap()
                .total_units,
            4
        );
        let slides = ToolOptions::PdfToPowerpoint(PdfToPowerPointOptions::default());
        assert_eq!(
            PdfToPowerPointTool::new(deps.clone())
                .validate(&[doc(6)], &slides)
                .unwrap()
                .total_units,
            7
        );
        let sheet = ToolOptions::PdfToExcel(PdfToExcelOptions::default());
        assert_eq!(
            PdfToExcelTool::new(deps.clone())
                .validate(&[doc(2)], &sheet)
                .unwrap()
                .total_units,
            3
        );
        let md = ToolOptions::PdfToMarkdown(PdfToMarkdownOptions::default());
        assert_eq!(
            PdfToMarkdownTool::new(deps.clone())
                .validate(&[doc(1)], &md)
                .unwrap()
                .total_units,
            2
        );
        let html = ToolOptions::PdfToHtml(PdfToHtmlOptions::default());
        assert_eq!(
            PdfToHtmlTool::new(deps)
                .validate(&[doc(1)], &html)
                .unwrap()
                .total_units,
            2
        );
    }

    #[test]
    fn bad_pages_languages_and_foreign_options_are_refused() {
        let deps = ToolDeps::unavailable();
        let tool = PdfToWordTool::new(deps.clone());
        let outside = ToolOptions::PdfToWord(PdfToWordOptions {
            pages: PageRange::parse("9").unwrap(),
            ..PdfToWordOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(3)], &outside).unwrap_err().code,
            ErrorCode::RangeOutOfBounds
        );
        let language = ToolOptions::PdfToWord(PdfToWordOptions {
            languages: vec!["??".to_owned()],
            ..PdfToWordOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(3)], &language).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        // Without OCR the languages do not matter.
        let off = ToolOptions::PdfToWord(PdfToWordOptions {
            languages: vec!["??".to_owned()],
            ocr: ExportOcr::Off,
            ..PdfToWordOptions::default()
        });
        assert!(tool.validate(&[doc(3)], &off).is_ok());
        let wrong = ToolOptions::Flip(domain::tools::FlipOptions::default());
        assert!(tool.validate(&[doc(1)], &wrong).is_err());
        assert!(
            PdfToExcelTool::new(deps.clone())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
        assert!(
            PdfToMarkdownTool::new(deps.clone())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
        assert!(
            PdfToHtmlTool::new(deps)
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
    }
}
