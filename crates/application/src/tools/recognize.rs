//! The tools that read scans: OCR (row 16), Deskew (row 17) and Scan to PDF (row 13).

use domain::tools::ToolGroup;
use domain::tools::recognize::{
    DeskewMode, DeskewOptions, OcrOptions, OcrOutput, ScanToPdfOptions, check_languages,
};
use domain::{Dpi, Duplicates, ErrorCode, PageIndex, ToolId, ToolOptions};

use super::common::{
    Progress, Scratch, ToolDeps, first_input, page_count, publish_edit, publish_pdf, wrong_options,
};
use crate::error::AppError;
use crate::ports::PdfEdit;
use crate::tool::{InputDoc, InputKind, InputMode, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions, units: u64) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: units,
    }
}

/// Resolution pages are measured at when looking for their tilt.
const MEASURE_DPI: u32 = 100;
/// A tilt smaller than this many degrees is left alone.
const MIN_TURN: f32 = 0.25;
/// Alphanumeric characters that make a page count as having text.
const TEXT_THRESHOLD: usize = 10;

fn selected_pages(range: &domain::PageRange, count: u32) -> Result<Vec<PageIndex>, AppError> {
    let mut pages: Vec<PageIndex> = range.resolve(count, Duplicates::Reject)?.pages().to_vec();
    pages.sort_by_key(|p| p.get());
    Ok(pages)
}

/// Checks that the recognizer and every language asked for are installed.
fn check_ready(deps: &ToolDeps, languages: &[String]) -> Result<(), AppError> {
    if !deps.ocr.is_available() {
        return Err(AppError::new(
            ErrorCode::ComponentMissing,
            "Text recognition is not installed. Open Settings, then Components, to install Tesseract and a language.",
        ));
    }
    let installed = deps.ocr.languages()?;
    if let Some(missing) = languages.iter().find(|l| !installed.contains(l)) {
        return Err(AppError::new(
            ErrorCode::ComponentMissing,
            format!(
                "The language \"{missing}\" is not installed. Open Settings, then Components, to install it."
            ),
        ));
    }
    Ok(())
}

fn has_text(deps: &ToolDeps, input: &InputDoc, page: PageIndex) -> Result<bool, AppError> {
    let text = deps
        .text
        .page_text(&input.path, input.password.as_ref(), page)?;
    Ok(text.chars().filter(|c| c.is_alphanumeric()).count() >= TEXT_THRESHOLD)
}

/// The turn that straightens a page, when it is tilted enough to be worth turning.
fn measure(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    page: PageIndex,
    max_angle: f32,
) -> Result<Option<f32>, AppError> {
    let image = deps.renderer.render_page(
        &input.path,
        input.password.as_ref(),
        page,
        Dpi::new(MEASURE_DPI)?,
    )?;
    ctx.cancel.check()?;
    Ok(deps
        .skew
        .skew(&image, max_angle)?
        .filter(|a| a.abs() >= MIN_TURN))
}

/// Reads a document and publishes it, or its text.
///
/// `naming` gives the output its name; `work` is what is read (for Scan to PDF that is the
/// scanned document, which has no file name of its own).
fn recognize_document(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    work: &InputDoc,
    naming: &InputDoc,
    label: &str,
    options: &OcrOptions,
    progress: &mut Progress<'_>,
) -> Result<OutputArtifact, AppError> {
    options.validate()?;
    check_ready(deps, &options.languages)?;
    let count = page_count(work);
    let wanted = selected_pages(&options.pages, count)?;

    // Pages that already have text are left alone, for a PDF; for text output they supply
    // their own text instead of being read again.
    let mut reading: Vec<PageIndex> = Vec::new();
    let mut existing: Vec<PageIndex> = Vec::new();
    for page in &wanted {
        ctx.cancel.check()?;
        if options.skip_pages_with_text && has_text(deps, work, *page)? {
            existing.push(*page);
        } else {
            reading.push(*page);
        }
    }

    // A tilted page is straightened first, on the page and in the picture that is read, by the
    // same angle about the same center, so the text lands where the words are.
    let mut turns: Vec<(PageIndex, f32)> = Vec::new();
    if options.deskew {
        for page in &reading {
            if let Some(angle) = measure(ctx, deps, work, *page, 20.0)? {
                turns.push((*page, angle));
            }
        }
    }
    let turn_of = |page: PageIndex| turns.iter().find(|(p, _)| *p == page).map(|(_, a)| *a);

    let read_page = |page: PageIndex| -> Result<crate::ports::RenderedImage, AppError> {
        let image =
            deps.renderer
                .render_page(&work.path, work.password.as_ref(), page, options.dpi)?;
        Ok(match turn_of(page) {
            Some(angle) => deps.skew.rotate(&image, angle)?,
            None => image,
        })
    };

    match options.output {
        OcrOutput::Text => {
            let mut text = String::new();
            for (i, page) in wanted.iter().enumerate() {
                ctx.cancel.check()?;
                if i > 0 {
                    text.push_str("\r\n");
                }
                text.push_str(&domain::tools::convert::page_separator(page.get() + 1));
                text.push_str("\r\n");
                let page_text = if reading.contains(page) {
                    let image = read_page(*page)?;
                    deps.ocr.recognize(
                        &image,
                        &options.languages,
                        options.dpi,
                        ctx.temp_dir,
                        ctx.cancel,
                    )?
                } else {
                    deps.text
                        .page_text(&work.path, work.password.as_ref(), *page)?
                };
                text.push_str(&windows_lines(page_text.trim_end()));
                text.push_str("\r\n");
                progress.advance(1);
            }
            let dest = ctx.output_path(naming, label, "txt", |_| {})?;
            let bytes = text.into_bytes();
            let path = ctx
                .files
                .write_atomic(&dest, &mut |w| w.write_all(&bytes), &|_| Ok(()))?;
            Ok(OutputArtifact {
                path,
                pages: None,
                bytes: bytes.len() as u64,
            })
        }
        OcrOutput::Pdf => {
            if reading.is_empty() {
                return Err(AppError::new(
                    ErrorCode::OptionInvalid,
                    "Every page you chose already has text, so there is nothing to read. Turn off \"skip pages that already have text\" to read them anyway.",
                ));
            }
            let mut layers: Vec<(PageIndex, std::path::PathBuf)> = Vec::new();
            let mut files: Vec<Scratch<'_>> = Vec::new();
            for page in &reading {
                ctx.cancel.check()?;
                let image = read_page(*page)?;
                let layer = Scratch::new(ctx, "pdf");
                deps.ocr.text_layer(
                    &image,
                    &options.languages,
                    options.dpi,
                    ctx.temp_dir,
                    layer.path(),
                    ctx.cancel,
                )?;
                layers.push((*page, layer.path().to_path_buf()));
                files.push(layer);
                progress.advance(1);
            }
            let mut steps = Vec::new();
            if !turns.is_empty() {
                steps.push(PdfEdit::Deskew(turns.clone()));
            }
            steps.push(PdfEdit::TextLayers(layers));
            let edit = if steps.len() == 1 {
                steps.remove(0)
            } else {
                PdfEdit::Sequence(steps)
            };
            publish_edit_named(ctx, deps, work, naming, label, count, &edit)
        }
    }
}

/// Like `publish_edit`, with the name taken from another input than the one edited.
fn publish_edit_named(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    work: &InputDoc,
    naming: &InputDoc,
    label: &str,
    expected_pages: u32,
    edit: &PdfEdit,
) -> Result<OutputArtifact, AppError> {
    if std::ptr::eq(work, naming) || work.path == naming.path {
        return publish_edit(ctx, deps, work, label, expected_pages, edit);
    }
    let protection = super::common::kept_protection(deps, work)?;
    let password = protection.as_ref().map(|p| &p.password);
    super::common::publish_pdf_checked(
        ctx,
        deps,
        naming,
        label,
        Some(expected_pages),
        password,
        &mut |tmp| super::common::edit_pipeline(ctx, deps, work, edit, protection.as_ref(), tmp),
    )
}

/// Line breaks as Windows programs expect them.
fn windows_lines(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

/// Reads scanned pages and makes them searchable, or writes their text.
pub struct OcrTool {
    deps: ToolDeps,
}

impl OcrTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for OcrTool {
    fn id(&self) -> ToolId {
        ToolId::Ocr
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Optimize
    }
    fn label(&self) -> &'static str {
        "ocr"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(OcrOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::Ocr(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        check_ready(&self.deps, &options.languages)?;
        Ok(plan_for(ToolId::Ocr, inputs, opts, pages.len() as u64))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::Ocr(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = recognize_document(
            ctx,
            &self.deps,
            input,
            input,
            self.label(),
            options,
            &mut progress,
        )?;
        Ok(vec![out])
    }
}

/// Straightens tilted pages.
pub struct DeskewTool {
    deps: ToolDeps,
}

impl DeskewTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for DeskewTool {
    fn id(&self) -> ToolId {
        ToolId::Deskew
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Optimize
    }
    fn label(&self) -> &'static str {
        "straightened"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(DeskewOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::Deskew(options) = opts else {
            return Err(wrong_options());
        };
        let input = first_input(inputs)?;
        let count = page_count(input);
        options.validate(count)?;
        let units = match options.mode {
            DeskewMode::Auto => selected_pages(&options.pages, count)?.len(),
            DeskewMode::Manual => options.angles.len(),
        };
        Ok(plan_for(ToolId::Deskew, inputs, opts, units as u64))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::Deskew(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let count = page_count(input);
        let mut progress = Progress::start(ctx, plan.total_units);
        let mut turns: Vec<(PageIndex, f32)> = Vec::new();
        match options.mode {
            DeskewMode::Manual => {
                for a in &options.angles {
                    turns.push((PageIndex::new(a.page - 1), a.degrees));
                    progress.advance(1);
                }
            }
            DeskewMode::Auto => {
                for page in selected_pages(&options.pages, count)? {
                    ctx.cancel.check()?;
                    if let Some(angle) =
                        measure(ctx, &self.deps, input, page, options.max_angle_deg)?
                    {
                        turns.push((page, angle));
                    }
                    progress.advance(1);
                }
            }
        }
        if turns.iter().all(|(_, a)| a.abs() < MIN_TURN) {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "No page is tilted enough to straighten, so nothing was changed. Use the manual mode to turn a page by an angle you choose.",
            ));
        }
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            count,
            &PdfEdit::Deskew(turns),
        )?;
        Ok(vec![out])
    }
}

/// Turns pictures of pages, from a scanner or a camera, into a PDF.
pub struct ScanToPdfTool {
    deps: ToolDeps,
}

impl ScanToPdfTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for ScanToPdfTool {
    fn id(&self) -> ToolId {
        ToolId::ScanToPdf
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertToPdf
    }
    fn label(&self) -> &'static str {
        "scan"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Image]
    }
    fn input_mode(&self) -> InputMode {
        InputMode::AllTogether
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ScanToPdfOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::ScanToPdf(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        first_input(inputs)?;
        if options.ocr {
            check_languages(&options.languages)?;
            check_ready(&self.deps, &options.languages)?;
        }
        let units = inputs.len() as u64 * if options.ocr { 2 } else { 1 };
        Ok(plan_for(ToolId::ScanToPdf, inputs, opts, units))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::ScanToPdf(options) = &plan.options else {
            return Err(wrong_options());
        };
        let first = first_input(&plan.inputs)?;
        let pages = plan.inputs.len() as u32;
        let mut progress = Progress::start(ctx, plan.total_units);
        let paths: Vec<std::path::PathBuf> = plan.inputs.iter().map(|i| i.path.clone()).collect();

        if !options.ocr {
            let out = publish_pdf(ctx, &self.deps, first, self.label(), pages, &mut |tmp| {
                self.deps
                    .scans
                    .scans_to_pdf(&paths, options, ctx.temp_dir, tmp, ctx.cancel)
                    .map(|_| ())
            })?;
            progress.advance(plan.total_units);
            return Ok(vec![out]);
        }

        // Make the PDF first, then read it like any scanned document.
        let scanned = Scratch::new(ctx, "pdf");
        self.deps
            .scans
            .scans_to_pdf(&paths, options, ctx.temp_dir, scanned.path(), ctx.cancel)?;
        progress.advance(u64::from(pages));
        let work = InputDoc {
            path: scanned.path().to_path_buf(),
            password: None,
            page_count: Some(pages),
        };
        let reading = OcrOptions {
            languages: options.languages.clone(),
            output: OcrOutput::Pdf,
            deskew: false,
            pages: domain::PageRange::all(),
            skip_pages_with_text: false,
            dpi: options.dpi,
        };
        let out = recognize_document(
            ctx,
            &self.deps,
            &work,
            first,
            self.label(),
            &reading,
            &mut progress,
        )?;
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::recognize::PageAngle;

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn deskew_counts_the_pages_it_will_measure_or_the_angles_typed() {
        let tool = DeskewTool::new(ToolDeps::unavailable());
        let auto = ToolOptions::Deskew(DeskewOptions::default());
        assert_eq!(tool.validate(&[doc(5)], &auto).unwrap().total_units, 5);
        let manual = ToolOptions::Deskew(DeskewOptions {
            mode: DeskewMode::Manual,
            angles: vec![PageAngle {
                page: 2,
                degrees: 1.5,
            }],
            ..DeskewOptions::default()
        });
        assert_eq!(tool.validate(&[doc(5)], &manual).unwrap().total_units, 1);
        let outside = ToolOptions::Deskew(DeskewOptions {
            mode: DeskewMode::Manual,
            angles: vec![PageAngle {
                page: 9,
                degrees: 1.5,
            }],
            ..DeskewOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(5)], &outside).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn ocr_without_the_recognizer_says_which_component_is_missing() {
        let tool = OcrTool::new(ToolDeps::unavailable());
        let err = tool
            .validate(&[doc(2)], &ToolOptions::Ocr(OcrOptions::default()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
        let bad = ToolOptions::Ocr(OcrOptions {
            languages: vec![],
            ..OcrOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(2)], &bad).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn scan_to_pdf_counts_a_unit_per_picture_and_per_page_read() {
        let tool = ScanToPdfTool::new(ToolDeps::unavailable());
        let pictures = [InputDoc::new("C:\\1.jpg"), InputDoc::new("C:\\2.jpg")];
        let plain = ToolOptions::ScanToPdf(ScanToPdfOptions::default());
        assert_eq!(tool.validate(&pictures, &plain).unwrap().total_units, 2);
        let with_ocr = ToolOptions::ScanToPdf(ScanToPdfOptions {
            ocr: true,
            ..ScanToPdfOptions::default()
        });
        assert_eq!(
            tool.validate(&pictures, &with_ocr).unwrap_err().code,
            ErrorCode::ComponentMissing
        );
    }

    #[test]
    fn windows_lines_unify_the_breaks() {
        assert_eq!(windows_lines("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    }
}
