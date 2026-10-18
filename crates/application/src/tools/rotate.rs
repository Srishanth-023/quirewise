//! Rotate (Appendix A row 32).

use domain::tools::RotateOptions;
use domain::tools::ToolGroup;
use domain::{ToolId, ToolOptions};

use super::common::{Progress, ToolDeps, first_input, page_count, publish_pdf, wrong_options};
use crate::error::AppError;
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Turns selected pages clockwise.
pub struct RotateTool {
    deps: ToolDeps,
}

impl RotateTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn options(opts: &ToolOptions) -> Result<&RotateOptions, AppError> {
    match opts {
        ToolOptions::Rotate(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for RotateTool {
    fn id(&self) -> ToolId {
        ToolId::Rotate
    }

    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }

    fn label(&self) -> &'static str {
        "rotated"
    }

    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }

    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(RotateOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let count = page_count(input);
        options(opts)?.select_pages(count)?;
        Ok(Plan {
            tool: ToolId::Rotate,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: u64::from(count),
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = options(&plan.options)?;
        let count = page_count(input);
        let selection = opts.select_pages(count)?;
        let rotations: Vec<_> = selection.pages().iter().map(|p| (*p, opts.angle)).collect();

        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(ctx, &self.deps, input, self.label(), count, &mut |tmp| {
            self.deps
                .writer
                .rotate(&input.path, input.password.as_ref(), &rotations, tmp)
        })?;
        progress.advance(u64::from(count));
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::PageRange;

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn validates_the_page_selection_against_the_document() {
        let tool = RotateTool::new(ToolDeps::unavailable());
        let ok = ToolOptions::Rotate(RotateOptions::default());
        assert_eq!(tool.validate(&[doc(5)], &ok).unwrap().total_units, 5);

        let out_of_range = ToolOptions::Rotate(RotateOptions {
            pages: PageRange::parse("9").unwrap(),
            ..RotateOptions::default()
        });
        let err = tool.validate(&[doc(5)], &out_of_range).unwrap_err();
        assert_eq!(err.code, domain::ErrorCode::RangeOutOfBounds);
    }

    #[test]
    fn describes_itself() {
        let tool = RotateTool::new(ToolDeps::unavailable());
        assert_eq!(tool.id(), ToolId::Rotate);
        assert_eq!(tool.accepts(), &[InputKind::Pdf]);
        assert!(tool.options_schema().as_value().is_object());
    }
}
