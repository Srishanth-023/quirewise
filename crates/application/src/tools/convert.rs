//! The conversion tools that need no outside program: Images to PDF (row 18), PDF to images
//! (row 23), Extract images (row 24), PDF to text (row 28) and TIFF to and from PDF (row 68).

use std::cell::Cell;
use std::path::PathBuf;

use domain::tools::ToolGroup;
use domain::tools::convert::{
    PdfToImagesOptions, PdfToTextOptions, PdfToTiffOptions, PictureColor, PictureFormat,
    TextLayout, page_separator,
};
use domain::{Duplicates, ErrorCode, PageIndex, ToolId, ToolOptions};

use super::common::{
    Progress, Scratch, ToolDeps, first_input, page_count, publish_file, publish_pdf,
    publish_pdf_checked, wrong_options,
};
use crate::error::AppError;
use crate::ports::{PictureSpec, TiffPages, TiffSpec};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions, units: u64) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: units,
    }
}

/// The pages a run reads, in page order.
fn selected_pages(range: &domain::PageRange, count: u32) -> Result<Vec<PageIndex>, AppError> {
    let mut pages: Vec<PageIndex> = range.resolve(count, Duplicates::Reject)?.pages().to_vec();
    pages.sort_by_key(|p| p.get());
    Ok(pages)
}

/// Puts pictures on PDF pages.
pub struct ImagesToPdfTool {
    deps: ToolDeps,
}

impl ImagesToPdfTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for ImagesToPdfTool {
    fn id(&self) -> ToolId {
        ToolId::ImagesToPdf
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertToPdf
    }
    fn label(&self) -> &'static str {
        "images"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Image]
    }
    fn input_mode(&self) -> crate::tool::InputMode {
        crate::tool::InputMode::AllTogether
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(domain::tools::ImagesToPdfOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        if !matches!(opts, ToolOptions::ImagesToPdf(_)) {
            return Err(wrong_options());
        }
        first_input(inputs)?;
        Ok(plan_for(
            ToolId::ImagesToPdf,
            inputs,
            opts,
            inputs.len() as u64,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::ImagesToPdf(options) = &plan.options else {
            return Err(wrong_options());
        };
        let mut progress = Progress::start(ctx, plan.total_units);
        let mut outputs = Vec::new();
        let groups: Vec<&[InputDoc]> = if options.one_pdf {
            vec![&plan.inputs[..]]
        } else {
            plan.inputs.chunks(1).collect()
        };
        for group in groups {
            ctx.cancel.check()?;
            let paths: Vec<PathBuf> = group.iter().map(|i| i.path.clone()).collect();
            let first = first_input(group)?;
            let pages = group.len() as u32;
            outputs.push(publish_pdf(
                ctx,
                &self.deps,
                first,
                self.label(),
                pages,
                &mut |tmp| {
                    self.deps
                        .images
                        .images_to_pdf(&paths, options, ctx.temp_dir, tmp)
                },
            )?);
            progress.advance(u64::from(pages));
        }
        Ok(outputs)
    }
}

/// Renders pages and writes them as picture files.
pub struct PdfToImagesTool {
    deps: ToolDeps,
}

impl PdfToImagesTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

/// Renders `pages` one at a time into an open TIFF.
fn render_into_tiff(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    pages: &[PageIndex],
    dpi: domain::Dpi,
    tiff: &mut dyn TiffPages,
    progress: &mut Progress<'_>,
) -> Result<(), AppError> {
    for page in pages {
        ctx.cancel.check()?;
        let image = deps
            .renderer
            .render_page(&input.path, input.password.as_ref(), *page, dpi)?;
        tiff.add(&image)?;
        progress.advance(1);
    }
    Ok(())
}

impl Tool for PdfToImagesTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToImages
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "images"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToImagesOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::PdfToImages(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        Ok(plan_for(
            ToolId::PdfToImages,
            inputs,
            opts,
            pages.len() as u64,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::PdfToImages(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let pictures = &self.deps.pictures;

        if options.format == PictureFormat::Tiff && options.single_tiff {
            let spec = TiffSpec {
                compression: domain::tools::convert::TiffCompression::Lzw,
                color: options.color,
                dpi: options.dpi,
            };
            let dest = ctx.output_path(input, self.label(), "tif", |_| {})?;
            let out = publish_file(
                ctx,
                &dest,
                &mut |tmp| {
                    let mut tiff = pictures.start_tiff(tmp, &spec)?;
                    render_into_tiff(
                        ctx,
                        &self.deps,
                        input,
                        &pages,
                        options.dpi,
                        tiff.as_mut(),
                        &mut progress,
                    )?;
                    tiff.finish()
                },
                &|tmp| pictures.check_picture(tmp, PictureFormat::Tiff, pages.len() as u32),
            )?;
            return Ok(vec![OutputArtifact {
                pages: Some(pages.len() as u32),
                ..out
            }]);
        }

        let spec = PictureSpec {
            format: options.format,
            jpeg_quality: options.jpeg_quality,
            color: options.color,
            dpi: options.dpi,
        };
        let mut outputs = Vec::with_capacity(pages.len());
        for page in &pages {
            ctx.cancel.check()?;
            let number = page.get() + 1;
            let dest = ctx.output_path(
                input,
                &format!("page {number}"),
                options.format.extension(),
                |name| name.page = Some(number),
            )?;
            outputs.push(publish_file(
                ctx,
                &dest,
                &mut |tmp| {
                    let image = self.deps.renderer.render_page(
                        &input.path,
                        input.password.as_ref(),
                        *page,
                        options.dpi,
                    )?;
                    pictures.write_picture(&image, &spec, tmp)
                },
                &|tmp| pictures.check_picture(tmp, options.format, 1),
            )?);
            progress.advance(1);
        }
        Ok(outputs)
    }
}

/// Takes the pictures out of a PDF.
pub struct ExtractImagesTool {
    deps: ToolDeps,
}

impl ExtractImagesTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for ExtractImagesTool {
    fn id(&self) -> ToolId {
        ToolId::ExtractImages
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "image"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(domain::tools::ExtractImagesOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::ExtractImages(_) = opts else {
            return Err(wrong_options());
        };
        let input = first_input(inputs)?;
        Ok(plan_for(
            ToolId::ExtractImages,
            inputs,
            opts,
            u64::from(page_count(input)),
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::ExtractImages(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let mut progress = Progress::start(ctx, plan.total_units);

        // The extractor reads plain files, so an encrypted document is unlocked first.
        let encrypted = self
            .deps
            .reader
            .open_info(&input.path, input.password.as_ref())?
            .encrypted;
        let unlocked = Scratch::new(ctx, "pdf");
        let source = if encrypted {
            self.deps
                .security
                .unlock(&input.path, input.password.as_ref(), unlocked.path())?;
            unlocked.path()
        } else {
            input.path.as_path()
        };

        let mut outputs: Vec<OutputArtifact> = Vec::new();
        let mut last_page = None;
        let report = self.deps.extractor.extract(
            source,
            options.keep_original_format,
            (options.min_width, options.min_height),
            &mut |image| {
                ctx.cancel.check()?;
                let number = image.page.get() + 1;
                if last_page != Some(number) {
                    // Pictures arrive in page order, so a new page finishes the one before.
                    let finished = last_page.map_or(0, |_| 1);
                    progress.advance(finished);
                    last_page = Some(number);
                }
                let dest = ctx.output_path(
                    input,
                    &format!("page {number} image {}", image.number),
                    image.kind.extension(),
                    |name| name.page = Some(number),
                )?;
                let bytes = &image.bytes;
                let path = ctx
                    .files
                    .write_atomic(&dest, &mut |w| w.write_all(bytes), &|_| Ok(()))?;
                outputs.push(OutputArtifact {
                    bytes: image.bytes.len() as u64,
                    path,
                    pages: None,
                });
                Ok(())
            },
        )?;
        progress.advance(plan.total_units);
        if outputs.is_empty() {
            return Err(no_pictures(&report, options.min_width, options.min_height));
        }
        Ok(outputs)
    }
}

fn no_pictures(report: &crate::ports::ExtractReport, min_w: u32, min_h: u32) -> AppError {
    let why = if report.found == 0 {
        "This document has no pictures. Pages that are scans hold one picture each; use PDF to images to get those pages as pictures."
            .to_owned()
    } else if report.too_small == report.found {
        format!(
            "All {} pictures are smaller than {min_w} by {min_h} pixels. Lower the size filter to get them.",
            report.found
        )
    } else {
        format!(
            "None of the {} pictures can be taken out as a file: {} are stored in a format that PDF to images handles better.",
            report.found, report.unsupported
        )
    };
    AppError::new(ErrorCode::OptionInvalid, why)
}

/// Writes the text of a document to a text file.
pub struct PdfToTextTool {
    deps: ToolDeps,
}

impl PdfToTextTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for PdfToTextTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToText
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "text"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToTextOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::PdfToText(options) = opts else {
            return Err(wrong_options());
        };
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        Ok(plan_for(
            ToolId::PdfToText,
            inputs,
            opts,
            pages.len() as u64,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::PdfToText(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        let mut progress = Progress::start(ctx, plan.total_units);

        let mut text = String::new();
        for (i, page) in pages.iter().enumerate() {
            ctx.cancel.check()?;
            if i > 0 {
                text.push_str("\r\n");
            }
            if options.page_separators {
                text.push_str(&page_separator(page.get() + 1));
                text.push_str("\r\n");
            }
            let page_text = match options.layout {
                TextLayout::Plain => {
                    self.deps
                        .text
                        .page_text(&input.path, input.password.as_ref(), *page)?
                }
                TextLayout::Layout => {
                    self.deps
                        .text
                        .page_text_layout(&input.path, input.password.as_ref(), *page)?
                }
            };
            text.push_str(&windows_lines(page_text.trim_end()));
            text.push_str("\r\n");
            progress.advance(1);
        }
        let bytes = options.encoding.encode(&text);
        let dest = ctx.output_path(input, self.label(), "txt", |_| {})?;
        let path = ctx
            .files
            .write_atomic(&dest, &mut |w| w.write_all(&bytes), &|_| Ok(()))?;
        Ok(vec![OutputArtifact {
            path,
            pages: None,
            bytes: bytes.len() as u64,
        }])
    }
}

/// Line breaks as Windows programs expect them, whatever the PDF used.
fn windows_lines(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    unified.replace('\n', "\r\n")
}

/// Writes every frame of a TIFF file as a PDF page.
pub struct TiffToPdfTool {
    deps: ToolDeps,
}

impl TiffToPdfTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for TiffToPdfTool {
    fn id(&self) -> ToolId {
        ToolId::TiffToPdf
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertToPdf
    }
    fn label(&self) -> &'static str {
        "tiff"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Image]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(domain::tools::TiffToPdfOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::TiffToPdf(_) = opts else {
            return Err(wrong_options());
        };
        first_input(inputs)?;
        Ok(plan_for(ToolId::TiffToPdf, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let pages = Cell::new(0u32);
        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            self.label(),
            None,
            None,
            &mut |tmp| {
                pages.set(self.deps.images.tiff_to_pdf(&input.path, tmp)?);
                Ok(())
            },
        )?;
        progress.advance(1);
        Ok(vec![OutputArtifact {
            pages: Some(pages.get()),
            ..out
        }])
    }
}

/// Writes pages as one multi-page TIFF file.
pub struct PdfToTiffTool {
    deps: ToolDeps,
}

impl PdfToTiffTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

impl Tool for PdfToTiffTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToTiff
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "tiff"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToTiffOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::PdfToTiff(options) = opts else {
            return Err(wrong_options());
        };
        let input = first_input(inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        Ok(plan_for(
            ToolId::PdfToTiff,
            inputs,
            opts,
            pages.len() as u64,
        ))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::PdfToTiff(options) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let pages = selected_pages(&options.pages, page_count(input))?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let spec = TiffSpec {
            compression: options.compression,
            color: if options.compression == domain::tools::convert::TiffCompression::CcittG4 {
                PictureColor::BlackWhite
            } else {
                options.color
            },
            dpi: options.dpi,
        };
        let pictures = &self.deps.pictures;
        let dest = ctx.output_path(input, self.label(), "tif", |_| {})?;
        let out = publish_file(
            ctx,
            &dest,
            &mut |tmp| {
                let mut tiff = pictures.start_tiff(tmp, &spec)?;
                render_into_tiff(
                    ctx,
                    &self.deps,
                    input,
                    &pages,
                    options.dpi,
                    tiff.as_mut(),
                    &mut progress,
                )?;
                tiff.finish()
            },
            &|tmp| pictures.check_picture(tmp, PictureFormat::Tiff, pages.len() as u32),
        )?;
        Ok(vec![OutputArtifact {
            pages: Some(pages.len() as u32),
            ..out
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::PageRange;
    use domain::tools::convert::{ExtractImagesOptions, ImagesToPdfOptions};

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn windows_lines_unifies_every_break_style() {
        assert_eq!(windows_lines("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn page_images_counts_the_selected_pages() {
        let tool = PdfToImagesTool::new(ToolDeps::unavailable());
        let options = ToolOptions::PdfToImages(PdfToImagesOptions {
            pages: PageRange::parse("2-4").unwrap(),
            ..PdfToImagesOptions::default()
        });
        assert_eq!(tool.validate(&[doc(6)], &options).unwrap().total_units, 3);
        let bad = ToolOptions::PdfToImages(PdfToImagesOptions {
            pages: PageRange::parse("9").unwrap(),
            ..PdfToImagesOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(6)], &bad).unwrap_err().code,
            ErrorCode::RangeOutOfBounds
        );
        let quality = ToolOptions::PdfToImages(PdfToImagesOptions {
            jpeg_quality: 0,
            ..PdfToImagesOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(6)], &quality).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn tools_reject_options_of_another_tool() {
        let wrong = ToolOptions::Flip(domain::tools::FlipOptions::default());
        assert!(
            ImagesToPdfTool::new(ToolDeps::unavailable())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
        assert!(
            ExtractImagesTool::new(ToolDeps::unavailable())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
        assert!(
            PdfToTextTool::new(ToolDeps::unavailable())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
    }

    #[test]
    fn images_to_pdf_counts_one_unit_per_picture() {
        let tool = ImagesToPdfTool::new(ToolDeps::unavailable());
        let options = ToolOptions::ImagesToPdf(ImagesToPdfOptions::default());
        let inputs = [InputDoc::new("C:\\a.png"), InputDoc::new("C:\\b.png")];
        assert_eq!(tool.validate(&inputs, &options).unwrap().total_units, 2);
        assert!(tool.validate(&[], &options).is_err());
    }

    #[test]
    fn extract_images_reports_why_nothing_came_out() {
        use crate::ports::ExtractReport;
        let none = no_pictures(&ExtractReport::default(), 50, 50);
        assert!(none.message.contains("no pictures"));
        let small = no_pictures(
            &ExtractReport {
                found: 3,
                too_small: 3,
                unsupported: 0,
            },
            50,
            50,
        );
        assert!(small.message.contains("smaller than 50 by 50"));
        let kinds = no_pictures(
            &ExtractReport {
                found: 2,
                too_small: 0,
                unsupported: 2,
            },
            50,
            50,
        );
        assert!(kinds.message.contains("PDF to images"));
        let _ = ExtractImagesOptions::default();
    }
}
