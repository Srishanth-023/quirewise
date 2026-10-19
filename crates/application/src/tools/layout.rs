//! The page layout tools: Crop (row 37), Resize (row 38), Flip (row 39), N-up (row 40) and
//! Grayscale (row 41).

use domain::tools::ToolGroup;
use domain::tools::common::FractionBox;
use domain::tools::layout::{
    CropMethod, CropOptions, FlipOptions, GrayscaleOptions, GrayscaleScope, NUpOptions,
    ResizeOptions, content_box, region_box,
};
use domain::{Dpi, Duplicates, PageIndex, ToolId, ToolOptions};

use super::common::{
    Progress, Scratch, ToolDeps, first_input, kept_protection, page_count, publish_edit,
    publish_pdf_checked, wrong_options,
};
use super::document::FlattenTool;
use crate::error::AppError;
use crate::ports::{PackageOptions, PdfEdit};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions, units: u64) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: units,
    }
}

/// Resolution of the pages rendered to find their content, in dots per inch.
const TRIM_DPI: u32 = 50;
/// Pixels lighter than this on every color channel count as paper.
const PAPER_THRESHOLD: u8 = 245;

/// Cuts pages down to a box.
pub struct CropTool {
    deps: ToolDeps,
}

impl CropTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn crop_options(opts: &ToolOptions) -> Result<&CropOptions, AppError> {
    match opts {
        ToolOptions::Crop(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for CropTool {
    fn id(&self) -> ToolId {
        ToolId::Crop
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "cropped"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(CropOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let options = crop_options(opts)?;
        options.validate()?;
        let selected = options
            .pages
            .resolve(page_count(input), Duplicates::Reject)?;
        Ok(plan_for(
            ToolId::Crop,
            inputs,
            opts,
            u64::try_from(selected.len()).unwrap_or(u64::MAX).max(1),
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = crop_options(&plan.options)?;
        let count = page_count(input);
        let mut pages: Vec<PageIndex> = options
            .pages
            .resolve(count, Duplicates::Reject)?
            .pages()
            .to_vec();
        pages.sort_by_key(|p| p.get());
        let sizes = self
            .deps
            .reader
            .page_sizes(&input.path, input.password.as_ref())?;

        let mut progress = Progress::start(ctx, plan.total_units);
        let mut crops: Vec<(PageIndex, FractionBox)> = Vec::with_capacity(pages.len());
        for page in pages {
            ctx.cancel.check()?;
            let size = sizes
                .get(page.get() as usize)
                .ok_or_else(|| AppError::internal("reading the page sizes"))?;
            let found = match options.method {
                CropMethod::Margins => Some(options.margin_box(size.width_pt, size.height_pt)?),
                CropMethod::Region => options.region.as_ref().map(region_box),
                CropMethod::Auto => {
                    let image = self.deps.renderer.render_page(
                        &input.path,
                        input.password.as_ref(),
                        page,
                        Dpi::new(TRIM_DPI)?,
                    )?;
                    content_box(
                        &image.rgba,
                        image.width,
                        image.height,
                        PAPER_THRESHOLD,
                        options.auto_padding_mm * 72.0 / 25.4,
                        size.width_pt,
                        size.height_pt,
                    )
                }
            };
            // A blank page has nothing to trim to and is left as it is.
            if let Some(b) = found {
                crops.push((page, b));
            }
            progress.advance(1);
        }
        if crops.is_empty() {
            return Err(domain::DomainError::option(
                "pages",
                "none of the selected pages has any content to crop to",
            )
            .into());
        }
        let edit = PdfEdit::CropPages {
            crops,
            media_box: options.apply_to_media_box,
        };
        Ok(vec![publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            count,
            &edit,
        )?])
    }
}

/// Gives pages a new size.
pub struct ResizeTool {
    deps: ToolDeps,
}

impl ResizeTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn resize_options(opts: &ToolOptions) -> Result<&ResizeOptions, AppError> {
    match opts {
        ToolOptions::Resize(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for ResizeTool {
    fn id(&self) -> ToolId {
        ToolId::Resize
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "resized"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ResizeOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        resize_options(opts)?.validate()?;
        Ok(plan_for(ToolId::Resize, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = resize_options(&plan.options)?;
        let (width_pt, height_pt) = options.target_points()?;
        let edit = PdfEdit::ResizePages {
            width_pt,
            height_pt,
            margin_pt: options.margin_pt(),
            scale: options.scale_content,
            match_orientation: options.match_orientation,
        };
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            page_count(input),
            &edit,
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Mirrors pages.
pub struct FlipTool {
    deps: ToolDeps,
}

impl FlipTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn flip_options(opts: &ToolOptions) -> Result<&FlipOptions, AppError> {
    match opts {
        ToolOptions::Flip(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for FlipTool {
    fn id(&self) -> ToolId {
        ToolId::Flip
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "flipped"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(FlipOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        flip_options(opts)?
            .pages
            .resolve(page_count(input), Duplicates::Reject)?;
        Ok(plan_for(ToolId::Flip, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = flip_options(&plan.options)?;
        let count = page_count(input);
        let mut pages: Vec<PageIndex> = options
            .pages
            .resolve(count, Duplicates::Reject)?
            .pages()
            .to_vec();
        pages.sort_by_key(|p| p.get());
        let edit = PdfEdit::Flip {
            direction: options.direction,
            pages,
        };
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_edit(ctx, &self.deps, input, self.label(), count, &edit)?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Puts several pages on each sheet.
pub struct NUpTool {
    deps: ToolDeps,
}

impl NUpTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn nup_options(opts: &ToolOptions) -> Result<&NUpOptions, AppError> {
    match opts {
        ToolOptions::NUp(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for NUpTool {
    fn id(&self) -> ToolId {
        ToolId::NUp
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "n-up"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(NUpOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        nup_options(opts)?;
        Ok(plan_for(ToolId::NUp, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = nup_options(&plan.options)?;
        let sheets = options.sheets(page_count(input)).max(1);
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            sheets,
            &PdfEdit::NUp(options.clone()),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Turns colors gray.
pub struct GrayscaleTool {
    deps: ToolDeps,
}

impl GrayscaleTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn gray_options(opts: &ToolOptions) -> Result<&GrayscaleOptions, AppError> {
    match opts {
        ToolOptions::Grayscale(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for GrayscaleTool {
    fn id(&self) -> ToolId {
        ToolId::Grayscale
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "grayscale"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(GrayscaleOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let options = gray_options(opts)?;
        // Unlock, the picture pass, the content pass and packing; a fallback renders pages first.
        let units = if options.raster_fallback {
            u64::from(page_count(input)) + 3
        } else {
            4
        };
        Ok(plan_for(ToolId::Grayscale, inputs, opts, units))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = gray_options(&plan.options)?;
        let count = page_count(input);
        let mut progress = Progress::start(ctx, plan.total_units);
        let protection = kept_protection(&self.deps, input)?;

        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            self.label(),
            Some(count),
            protection.as_ref().map(|p| &p.password),
            &mut |tmp| {
                // Every stage reads the previous stage's file; `current` always holds the latest.
                let mut stages: Vec<Scratch<'_>> = Vec::new();
                let mut current = Scratch::new(ctx, "pdf");
                if options.raster_fallback {
                    FlattenTool::new(self.deps.clone()).flatten_everything(
                        ctx,
                        input,
                        options.dpi.value(),
                        &mut progress,
                        current.path(),
                    )?;
                } else {
                    self.deps.security.unlock(
                        &input.path,
                        input.password.as_ref(),
                        current.path(),
                    )?;
                    progress.advance(1);
                }
                ctx.cancel.check()?;
                let images = options.raster_fallback || options.apply_to != GrayscaleScope::Text;
                if images {
                    let next = Scratch::new(ctx, "pdf");
                    self.deps
                        .optimizer
                        .grayscale_images(current.path(), next.path())?;
                    stages.push(std::mem::replace(&mut current, next));
                }
                progress.advance(1);
                ctx.cancel.check()?;
                let content =
                    !options.raster_fallback && options.apply_to != GrayscaleScope::Images;
                if content {
                    let next = Scratch::new(ctx, "pdf");
                    self.deps.editor.apply(
                        current.path(),
                        &PdfEdit::GrayscaleContent,
                        next.path(),
                    )?;
                    stages.push(std::mem::replace(&mut current, next));
                }
                progress.advance(1);
                let packaging = PackageOptions {
                    keep_protection: protection.clone(),
                    ..PackageOptions::new(false, true)
                };
                self.deps
                    .packager
                    .package(current.path(), None, &packaging, tmp)
            },
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ErrorCode;
    use domain::tools::layout::{Margins, PaperSize};

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn crop_needs_something_to_cut_and_pages_that_exist() {
        let tool = CropTool::new(ToolDeps::unavailable());
        let none = ToolOptions::defaults_for(ToolId::Crop);
        assert_eq!(
            tool.validate(&[doc(3)], &none).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let ok = ToolOptions::Crop(CropOptions {
            margins: Margins {
                left: 10.0,
                ..Margins::default()
            },
            ..CropOptions::default()
        });
        assert_eq!(tool.validate(&[doc(3)], &ok).unwrap().total_units, 3);
        let bad_pages = ToolOptions::Crop(CropOptions {
            pages: domain::PageRange::parse("8").unwrap(),
            margins: Margins {
                left: 10.0,
                ..Margins::default()
            },
            ..CropOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(3)], &bad_pages).unwrap_err().code,
            ErrorCode::RangeOutOfBounds
        );
        let auto = ToolOptions::Crop(CropOptions {
            method: CropMethod::Auto,
            ..CropOptions::default()
        });
        assert!(tool.validate(&[doc(2)], &auto).is_ok());
    }

    #[test]
    fn resize_checks_the_custom_size() {
        let tool = ResizeTool::new(ToolDeps::unavailable());
        assert!(
            tool.validate(&[doc(1)], &ToolOptions::defaults_for(ToolId::Resize))
                .is_ok()
        );
        let bad = ToolOptions::Resize(ResizeOptions {
            size: PaperSize::Custom,
            custom_width_mm: 1.0,
            ..ResizeOptions::default()
        });
        assert!(tool.validate(&[doc(1)], &bad).is_err());
    }

    #[test]
    fn flip_checks_pages_and_n_up_accepts_its_defaults() {
        let flip = FlipTool::new(ToolDeps::unavailable());
        let bad = ToolOptions::Flip(FlipOptions {
            pages: domain::PageRange::parse("5").unwrap(),
            ..FlipOptions::default()
        });
        assert_eq!(
            flip.validate(&[doc(2)], &bad).unwrap_err().code,
            ErrorCode::RangeOutOfBounds
        );
        let nup = NUpTool::new(ToolDeps::unavailable());
        assert!(
            nup.validate(&[doc(2)], &ToolOptions::defaults_for(ToolId::NUp))
                .is_ok()
        );
    }

    #[test]
    fn grayscale_progress_covers_every_stage_and_each_page_of_a_fallback() {
        let tool = GrayscaleTool::new(ToolDeps::unavailable());
        let plain = tool
            .validate(&[doc(6)], &ToolOptions::defaults_for(ToolId::Grayscale))
            .unwrap();
        assert_eq!(plain.total_units, 4);
        let fallback = ToolOptions::Grayscale(GrayscaleOptions {
            raster_fallback: true,
            ..GrayscaleOptions::default()
        });
        assert_eq!(tool.validate(&[doc(6)], &fallback).unwrap().total_units, 9);
    }
}
