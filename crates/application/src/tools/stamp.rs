//! The stamping tools: Page numbers (row 33), Header and footer (row 34), Bates numbering
//! (row 35) and Watermark (row 36). They share the stamper: a tool decides what to draw where,
//! the engine draws it.

use domain::PageIndex;
use domain::stamp::{PageStamps, Stamp};
use domain::tools::ToolGroup;
use domain::tools::stamp::{
    BatesOptions, FileFacts, HeaderFooterOptions, PageNumbersOptions, WatermarkKind,
    WatermarkOptions,
};
use domain::{Duplicates, ErrorCode, ToolId, ToolOptions};

use super::common::{Progress, ToolDeps, first_input, page_count, publish_edit, wrong_options};
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

/// Today as `2026-10-19`.
fn today(ctx: &JobContext<'_>) -> String {
    let now = ctx.clock.now();
    format!("{:04}-{:02}-{:02}", now.year, now.month, now.day)
}

/// A date for checking templates before a run starts.
const SAMPLE_DATE: &str = "2000-01-01";

fn no_pages() -> AppError {
    domain::DomainError::option(
        "pages",
        "none of the selected pages exists in this document",
    )
    .into()
}

/// Draws a number on each page.
pub struct PageNumbersTool {
    deps: ToolDeps,
}

impl PageNumbersTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn numbers_options(opts: &ToolOptions) -> Result<&PageNumbersOptions, AppError> {
    match opts {
        ToolOptions::PageNumbers(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for PageNumbersTool {
    fn id(&self) -> ToolId {
        ToolId::PageNumbers
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "numbered"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PageNumbersOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let options = numbers_options(opts)?;
        let stem = input.stem();
        let plan = options.plan(&FileFacts {
            page_count: page_count(input),
            filename: &stem,
            date: SAMPLE_DATE,
        })?;
        if plan.is_empty() {
            return Err(no_pages());
        }
        Ok(plan_for(ToolId::PageNumbers, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = numbers_options(&plan.options)?;
        let stem = input.stem();
        let stamps = options.plan(&FileFacts {
            page_count: page_count(input),
            filename: &stem,
            date: &today(ctx),
        })?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            page_count(input),
            &PdfEdit::Stamp(stamps),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Draws header and footer texts.
pub struct HeaderFooterTool {
    deps: ToolDeps,
}

impl HeaderFooterTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn header_options(opts: &ToolOptions) -> Result<&HeaderFooterOptions, AppError> {
    match opts {
        ToolOptions::HeaderFooter(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for HeaderFooterTool {
    fn id(&self) -> ToolId {
        ToolId::HeaderFooter
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "header and footer"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(HeaderFooterOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let options = header_options(opts)?;
        let stem = input.stem();
        let plan = options.plan(&FileFacts {
            page_count: page_count(input),
            filename: &stem,
            date: SAMPLE_DATE,
        })?;
        if plan.is_empty() {
            return Err(no_pages());
        }
        Ok(plan_for(ToolId::HeaderFooter, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = header_options(&plan.options)?;
        let stem = input.stem();
        let stamps = options.plan(&FileFacts {
            page_count: page_count(input),
            filename: &stem,
            date: &today(ctx),
        })?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            page_count(input),
            &PdfEdit::Stamp(stamps),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Numbers every page with a prefix, padded digits and a suffix, across several files.
pub struct BatesTool {
    deps: ToolDeps,
}

impl BatesTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn bates_options(opts: &ToolOptions) -> Result<&BatesOptions, AppError> {
    match opts {
        ToolOptions::Bates(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for BatesTool {
    fn id(&self) -> ToolId {
        ToolId::Bates
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "bates numbered"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    /// All files run together, in list order, so the numbers can continue from one to the next.
    fn input_mode(&self) -> InputMode {
        InputMode::AllTogether
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(BatesOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        bates_options(opts)?.validate()?;
        let units = inputs.iter().map(|i| u64::from(page_count(i))).sum();
        Ok(plan_for(ToolId::Bates, inputs, opts, units))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let options = bates_options(&plan.options)?;
        let counts: Vec<u32> = plan.inputs.iter().map(page_count).collect();
        let starts = options.starts(&counts);
        let mut progress = Progress::start(ctx, plan.total_units);
        let mut outputs = Vec::with_capacity(plan.inputs.len());
        for ((input, count), start) in plan.inputs.iter().zip(counts).zip(starts) {
            ctx.cancel.check()?;
            let stamps = options.plan(count, start)?;
            outputs.push(publish_edit(
                ctx,
                &self.deps,
                input,
                self.label(),
                count,
                &PdfEdit::Stamp(stamps),
            )?);
            progress.advance(u64::from(count));
        }
        Ok(outputs)
    }
}

/// Draws a text or picture on pages.
pub struct WatermarkTool {
    deps: ToolDeps,
}

impl WatermarkTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn watermark_options(opts: &ToolOptions) -> Result<&WatermarkOptions, AppError> {
    match opts {
        ToolOptions::Watermark(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

/// PNG and JPEG signatures.
fn is_picture(head: &[u8]) -> bool {
    head.starts_with(&[0x89, b'P', b'N', b'G']) || head.starts_with(&[0xFF, 0xD8, 0xFF])
}

impl Tool for WatermarkTool {
    fn id(&self) -> ToolId {
        ToolId::Watermark
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "watermarked"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(WatermarkOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let options = watermark_options(opts)?;
        options.stamp()?;
        options
            .pages
            .resolve(page_count(input), Duplicates::Reject)?;
        Ok(plan_for(ToolId::Watermark, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = watermark_options(&plan.options)?;
        let stamp: Stamp = options.stamp()?;
        if options.kind == WatermarkKind::Image
            && let Some(path) = &options.image_path
        {
            let head = ctx.files.read_head(path, 8)?;
            if !is_picture(&head) {
                return Err(AppError::new(
                    ErrorCode::OptionInvalid,
                    "The watermark picture is not a PNG or JPEG file. Choose another picture.",
                ));
            }
        }
        let count = page_count(input);
        let mut pages: Vec<PageIndex> = options
            .pages
            .resolve(count, Duplicates::Reject)?
            .pages()
            .to_vec();
        pages.sort_by_key(|p| p.get());
        let stamps: Vec<PageStamps> = pages
            .into_iter()
            .map(|page| PageStamps {
                page,
                stamps: vec![stamp.clone()],
            })
            .collect();
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            count,
            &PdfEdit::Stamp(stamps),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::stamp::Position;
    use domain::tools::stamp::WatermarkKind;
    use std::path::PathBuf;

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn page_numbers_check_template_and_pages_before_any_work() {
        let tool = PageNumbersTool::new(ToolDeps::unavailable());
        let ok = ToolOptions::defaults_for(ToolId::PageNumbers);
        assert_eq!(
            tool.validate(&[doc(3)], &ok).unwrap().tool,
            ToolId::PageNumbers
        );
        let bad = ToolOptions::PageNumbers(PageNumbersOptions {
            template: "{nope}".into(),
            ..PageNumbersOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(3)], &bad).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let out_of_range = ToolOptions::PageNumbers(PageNumbersOptions {
            pages: domain::PageRange::parse("9").unwrap(),
            ..PageNumbersOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(3)], &out_of_range).unwrap_err().code,
            ErrorCode::RangeOutOfBounds
        );
        let only_cover = ToolOptions::PageNumbers(PageNumbersOptions {
            skip_first_page: true,
            ..PageNumbersOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(1)], &only_cover).unwrap_err().code,
            ErrorCode::OptionInvalid,
            "nothing is left to number"
        );
    }

    #[test]
    fn header_and_footer_need_text() {
        let tool = HeaderFooterTool::new(ToolDeps::unavailable());
        let empty = ToolOptions::defaults_for(ToolId::HeaderFooter);
        assert_eq!(
            tool.validate(&[doc(2)], &empty).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let ok = ToolOptions::HeaderFooter(HeaderFooterOptions {
            footer_center: "{page}".into(),
            ..HeaderFooterOptions::default()
        });
        assert!(tool.validate(&[doc(2)], &ok).is_ok());
    }

    #[test]
    fn bates_runs_all_files_together_and_counts_pages_as_progress() {
        let tool = BatesTool::new(ToolDeps::unavailable());
        assert_eq!(tool.input_mode(), InputMode::AllTogether);
        let plan = tool
            .validate(&[doc(3), doc(4)], &ToolOptions::defaults_for(ToolId::Bates))
            .unwrap();
        assert_eq!(plan.total_units, 7);
        let bad = ToolOptions::Bates(BatesOptions {
            digits: 0,
            ..BatesOptions::default()
        });
        assert!(tool.validate(&[doc(1)], &bad).is_err());
    }

    #[test]
    fn watermark_needs_its_text_or_picture() {
        let tool = WatermarkTool::new(ToolDeps::unavailable());
        assert!(
            tool.validate(&[doc(2)], &ToolOptions::defaults_for(ToolId::Watermark))
                .is_ok()
        );
        let no_picture = ToolOptions::Watermark(WatermarkOptions {
            kind: WatermarkKind::Image,
            ..WatermarkOptions::default()
        });
        assert!(tool.validate(&[doc(2)], &no_picture).is_err());
        let with_picture = ToolOptions::Watermark(WatermarkOptions {
            kind: WatermarkKind::Image,
            image_path: Some(PathBuf::from("C:\\logo.png")),
            position: Position::TopLeft,
            ..WatermarkOptions::default()
        });
        assert!(tool.validate(&[doc(2)], &with_picture).is_ok());
    }

    #[test]
    fn picture_signatures_are_png_and_jpeg() {
        assert!(is_picture(&[0x89, b'P', b'N', b'G', 0x0D]));
        assert!(is_picture(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!is_picture(b"GIF89a"));
        assert!(!is_picture(b"%PDF-1.7"));
    }
}
