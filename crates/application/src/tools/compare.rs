//! Compare (row 55): what differs between two versions of a document, as a report.
//!
//! The report is a PDF: a summary, the text changes of the two files side by side, and a picture
//! of every page that looks different, with what is only in the original in red and what is only
//! in the changed file in blue.

use domain::tools::ToolGroup;
use domain::tools::compare::CompareOptions;
use domain::tools::convert::{PictureColor, PictureFormat};
use domain::{ErrorCode, PageIndex, ToolId, ToolOptions};

use super::common::{Progress, Scratch, ToolDeps, page_count, publish_pdf_checked, wrong_options};
use super::comparediff::{Line, diff_pixels, diff_text, lines_of};
use super::comparereport::{PAGE, Summary, VisualPage, layout};
use crate::error::AppError;
use crate::ports::{AssemblyPlan, PdfEdit, PictureSpec, PlannedPage, RenderedImage};
use crate::tool::{InputDoc, InputKind, InputMode, JobContext, OutputArtifact, Plan, Tool};

/// Compares two documents.
pub struct CompareTool {
    deps: ToolDeps,
}

impl CompareTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }

    fn name_of(input: &InputDoc) -> String {
        input.path.file_name().map_or_else(
            || "document".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    fn render(
        &self,
        input: &InputDoc,
        page: u32,
        dpi: domain::Dpi,
    ) -> Result<Option<RenderedImage>, AppError> {
        if page >= page_count(input) {
            return Ok(None);
        }
        self.deps
            .renderer
            .render_page(
                &input.path,
                input.password.as_ref(),
                PageIndex::new(page),
                dpi,
            )
            .map(Some)
    }

    fn lines(
        &self,
        input: &InputDoc,
        page: u32,
        ignore_whitespace: bool,
    ) -> Result<Vec<Line>, AppError> {
        if page >= page_count(input) {
            return Ok(Vec::new());
        }
        let text =
            self.deps
                .text
                .page_text(&input.path, input.password.as_ref(), PageIndex::new(page))?;
        Ok(lines_of(page + 1, &text, ignore_whitespace))
    }
}

impl Tool for CompareTool {
    fn id(&self) -> ToolId {
        ToolId::Compare
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Security
    }
    fn label(&self) -> &'static str {
        "compared"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn input_mode(&self) -> InputMode {
        InputMode::AllTogether
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(CompareOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::Compare(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let [original, changed] = inputs else {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "Compare needs two PDF files: the original first, then the changed one.",
            ));
        };
        let longest = page_count(original).max(page_count(changed));
        let pages = options.select_pages(longest)?.len() as u64;
        let per_page = 1 + u64::from(options.mode.visual()) * 2;
        Ok(Plan {
            tool: ToolId::Compare,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: pages * per_page + 2,
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::Compare(options) = &plan.options else {
            return Err(wrong_options());
        };
        let [original, changed] = &plan.inputs[..] else {
            return Err(wrong_options());
        };
        let longest = page_count(original).max(page_count(changed));
        let pages = options.select_pages(longest)?;
        let dpi = options.resolution()?;
        let tolerance = options.tolerance_levels();
        let mut progress = Progress::start(ctx, plan.total_units);

        let mut old_lines: Vec<Line> = Vec::new();
        let mut new_lines: Vec<Line> = Vec::new();
        let mut visual: Vec<VisualPage> = Vec::new();
        // The pictures stay on disk until the report is written.
        let mut pictures: Vec<Scratch<'_>> = Vec::new();
        let spec = PictureSpec {
            format: PictureFormat::Png,
            jpeg_quality: 90,
            color: PictureColor::Color,
            dpi,
        };
        for page in pages.pages() {
            ctx.cancel.check()?;
            let n = page.get();
            if options.mode.text() {
                old_lines.extend(self.lines(original, n, options.ignore_whitespace)?);
                new_lines.extend(self.lines(changed, n, options.ignore_whitespace)?);
            }
            progress.advance(1);
            if options.mode.visual() {
                let a = self.render(original, n, dpi)?;
                progress.advance(1);
                ctx.cancel.check()?;
                let b = self.render(changed, n, dpi)?;
                let diff = diff_pixels(a.as_ref(), b.as_ref(), tolerance);
                if diff.changed > 0 {
                    let file = Scratch::new(ctx, "png");
                    self.deps
                        .pictures
                        .write_picture(&diff.overlay, &spec, file.path())?;
                    visual.push(VisualPage {
                        page: n + 1,
                        percent: diff.percent(),
                        size_differs: diff.size_differs,
                        image: file.path().to_path_buf(),
                    });
                    pictures.push(file);
                }
                progress.advance(1);
            }
        }

        let text = options
            .mode
            .text()
            .then(|| diff_text(&old_lines, &new_lines));
        let summary = Summary {
            original_name: Self::name_of(original),
            changed_name: Self::name_of(changed),
            original_pages: page_count(original),
            changed_pages: page_count(changed),
            compared_pages: pages.len(),
        };
        let report = layout(&summary, text.as_ref(), options.mode.visual(), &visual);
        progress.advance(1);

        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            original,
            self.label(),
            Some(report.pages),
            None,
            &mut |tmp| {
                let blank = Scratch::new(ctx, "pdf");
                let sheets = AssemblyPlan {
                    sources: Vec::new(),
                    pages: (0..report.pages)
                        .map(|_| PlannedPage::Blank {
                            width_pt: PAGE.0,
                            height_pt: PAGE.1,
                        })
                        .collect(),
                };
                self.deps
                    .writer
                    .assemble(&sheets, ctx.temp_dir, blank.path())?;
                ctx.cancel.check()?;
                self.deps
                    .editor
                    .apply(blank.path(), &PdfEdit::Overlay(report.items.clone()), tmp)
            },
        )?;
        drop(pictures);
        progress.advance(1);
        Ok(vec![out])
    }
}
