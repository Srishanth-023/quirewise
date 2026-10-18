//! The document tools: Remove annotations (row 44), Flatten (row 47) and Edit metadata (row 48).

use domain::tools::ToolGroup;
use domain::tools::document::{
    FlattenMode, FlattenOptions, MetadataOptions, RemoveAnnotationsOptions,
};
use domain::{PageIndex, ToolId, ToolOptions};

use super::common::{
    Progress, Scratch, ToolDeps, edit_pipeline, first_input, page_count, publish_pdf, wrong_options,
};
use crate::error::AppError;
use crate::ports::{PageImage, PdfEdit};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Removes annotations of chosen kinds from chosen pages.
pub struct RemoveAnnotationsTool {
    deps: ToolDeps,
}

impl RemoveAnnotationsTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn annotation_options(opts: &ToolOptions) -> Result<&RemoveAnnotationsOptions, AppError> {
    match opts {
        ToolOptions::RemoveAnnotations(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for RemoveAnnotationsTool {
    fn id(&self) -> ToolId {
        ToolId::RemoveAnnotations
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "annotations removed"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(RemoveAnnotationsOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        annotation_options(opts)?.select(page_count(input))?;
        Ok(Plan {
            tool: ToolId::RemoveAnnotations,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: 1,
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = annotation_options(&plan.options)?;
        let count = page_count(input);
        let pages: Vec<PageIndex> = opts.select(count)?.pages().to_vec();
        let edit = PdfEdit::RemoveAnnotations {
            kinds: opts.kinds.clone(),
            pages,
        };
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(ctx, &self.deps, input, self.label(), count, &mut |tmp| {
            edit_pipeline(ctx, &self.deps, input, &edit, tmp)
        })?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Edits the document information and keeps the XMP packet in step.
pub struct EditMetadataTool {
    deps: ToolDeps,
}

impl EditMetadataTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn metadata_options(opts: &ToolOptions) -> Result<&MetadataOptions, AppError> {
    match opts {
        ToolOptions::EditMetadata(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for EditMetadataTool {
    fn id(&self) -> ToolId {
        ToolId::EditMetadata
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "metadata edited"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(MetadataOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        metadata_options(opts)?.change()?;
        Ok(Plan {
            tool: ToolId::EditMetadata,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: 1,
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let edit = PdfEdit::SetMetadata(metadata_options(&plan.options)?.change()?);
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(
            ctx,
            &self.deps,
            input,
            self.label(),
            page_count(input),
            &mut |tmp| edit_pipeline(ctx, &self.deps, input, &edit, tmp),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Turns form fields and annotations, or whole pages, into plain page content.
pub struct FlattenTool {
    deps: ToolDeps,
}

impl FlattenTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }

    /// Renders every page to a picture and builds a document from the pictures.
    fn flatten_everything(
        &self,
        ctx: &JobContext<'_>,
        input: &InputDoc,
        dpi: u32,
        progress: &mut Progress<'_>,
        tmp: &std::path::Path,
    ) -> Result<(), AppError> {
        let sizes = self
            .deps
            .reader
            .page_sizes(&input.path, input.password.as_ref())?;
        let mut files = Vec::with_capacity(sizes.len());
        let mut pages = Vec::with_capacity(sizes.len());
        for (i, size) in sizes.iter().enumerate() {
            ctx.cancel.check()?;
            let index =
                PageIndex::new(u32::try_from(i).map_err(|_| AppError::internal("counting pages"))?);
            let longest = size.width_pt.max(size.height_pt);
            let max_edge = ((longest / 72.0 * dpi as f32).ceil() as u32).max(1);
            let image = self.deps.renderer.render_png(
                &input.path,
                input.password.as_ref(),
                index,
                max_edge,
            )?;
            // The picture shows the page as displayed; the size says which way round that is.
            let landscape = image.width >= image.height;
            let (w, h) = if landscape == (size.width_pt >= size.height_pt) {
                (size.width_pt, size.height_pt)
            } else {
                (size.height_pt, size.width_pt)
            };
            let scratch = Scratch::new(ctx, "png");
            ctx.files.write_atomic(
                scratch.path(),
                &mut |out| out.write_all(&image.png),
                &|_| Ok(()),
            )?;
            pages.push(PageImage {
                png: scratch.path().to_path_buf(),
                width_pt: w,
                height_pt: h,
            });
            files.push(scratch);
            progress.advance(1);
        }
        self.deps.page_images.pages_to_pdf(&pages, tmp)
    }
}

fn flatten_options(opts: &ToolOptions) -> Result<&FlattenOptions, AppError> {
    match opts {
        ToolOptions::Flatten(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for FlattenTool {
    fn id(&self) -> ToolId {
        ToolId::Flatten
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Forms
    }
    fn label(&self) -> &'static str {
        "flattened"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(FlattenOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        flatten_options(opts)?;
        Ok(Plan {
            tool: ToolId::Flatten,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: u64::from(page_count(input).max(1)),
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = flatten_options(&plan.options)?;
        let count = page_count(input);
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(
            ctx,
            &self.deps,
            input,
            self.label(),
            count,
            &mut |tmp| match opts.mode {
                FlattenMode::FormsAndAnnotations => {
                    self.deps
                        .flattener
                        .flatten_forms(&input.path, input.password.as_ref(), tmp)?;
                    progress.advance(u64::from(count));
                    Ok(())
                }
                FlattenMode::Everything => {
                    self.flatten_everything(ctx, input, opts.dpi.value(), &mut progress, tmp)
                }
            },
        )?;
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ErrorCode;
    use domain::PageRange;
    use domain::tools::document::AnnotationKind;

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn annotation_removal_checks_kinds_and_pages() {
        let tool = RemoveAnnotationsTool::new(ToolDeps::unavailable());
        assert!(
            tool.validate(
                &[doc(4)],
                &ToolOptions::defaults_for(ToolId::RemoveAnnotations)
            )
            .is_ok()
        );
        let nothing = ToolOptions::RemoveAnnotations(RemoveAnnotationsOptions {
            kinds: vec![],
            pages: PageRange::all(),
        });
        assert_eq!(
            tool.validate(&[doc(4)], &nothing).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let far = ToolOptions::RemoveAnnotations(RemoveAnnotationsOptions {
            kinds: vec![AnnotationKind::Links],
            pages: PageRange::parse("9").unwrap(),
        });
        assert_eq!(
            tool.validate(&[doc(4)], &far).unwrap_err().code,
            ErrorCode::RangeOutOfBounds
        );
    }

    #[test]
    fn metadata_needs_a_change_and_good_dates() {
        let tool = EditMetadataTool::new(ToolDeps::unavailable());
        let empty = ToolOptions::defaults_for(ToolId::EditMetadata);
        assert!(tool.validate(&[doc(1)], &empty).is_err());
        let bad_date = ToolOptions::EditMetadata(MetadataOptions {
            creation_date: Some("2026-02-30".into()),
            ..MetadataOptions::default()
        });
        assert!(tool.validate(&[doc(1)], &bad_date).is_err());
        let ok = ToolOptions::EditMetadata(MetadataOptions {
            title: Some("T".into()),
            ..MetadataOptions::default()
        });
        assert!(tool.validate(&[doc(1)], &ok).is_ok());
    }

    #[test]
    fn flatten_progress_follows_the_page_count() {
        let tool = FlattenTool::new(ToolDeps::unavailable());
        let plan = tool
            .validate(&[doc(7)], &ToolOptions::defaults_for(ToolId::Flatten))
            .unwrap();
        assert_eq!(plan.total_units, 7);
    }
}
