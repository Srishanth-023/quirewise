//! Insert blank or duplicate pages (Appendix A row 69).

use domain::planners::{InsertStep, insert_plan};
use domain::tools::common::RightAngle;
use domain::tools::pages::InsertMode;
use domain::tools::{InsertPagesOptions, ToolGroup};
use domain::{PageIndex, ToolId, ToolOptions};

use super::common::{Progress, ToolDeps, first_input, page_count, publish_pdf, wrong_options};
use crate::error::AppError;
use crate::ports::{AssemblyPlan, PageSize, PlannedPage, SourceDoc};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Inserts blank pages or copies of pages.
pub struct InsertPagesTool {
    deps: ToolDeps,
}

impl InsertPagesTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn options(opts: &ToolOptions) -> Result<&InsertPagesOptions, AppError> {
    match opts {
        ToolOptions::InsertPages(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for InsertPagesTool {
    fn id(&self) -> ToolId {
        ToolId::InsertPages
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "pages inserted"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(InsertPagesOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        options(opts)?.selection(page_count(input))?;
        Ok(Plan {
            tool: ToolId::InsertPages,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: u64::from(page_count(input)),
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let o = options(&plan.options)?;
        let count = page_count(input);
        let selected = o.selection(count)?;
        let steps = insert_plan(count, &selected, o.kind(), o.where_(), o.count);

        // Blank pages take the size of the neighboring page unless a size was chosen.
        let sizes: Vec<PageSize> = if o.page_size.points().is_none() && o.mode == InsertMode::Blank
        {
            self.deps
                .reader
                .page_sizes(&input.path, input.password.as_ref())?
        } else {
            Vec::new()
        };
        let pages: Vec<PlannedPage> = steps
            .iter()
            .map(|step| match *step {
                InsertStep::Original(page) | InsertStep::Copy(page) => PlannedPage::Source {
                    source: 0,
                    page: PageIndex::new(page),
                    rotate: RightAngle::Deg0,
                },
                InsertStep::Blank { neighbor } => {
                    let (width_pt, height_pt) = o.page_size.points().unwrap_or_else(|| {
                        sizes
                            .get(neighbor as usize)
                            .map_or((612.0, 792.0), |s| (s.width_pt, s.height_pt))
                    });
                    PlannedPage::Blank {
                        width_pt,
                        height_pt,
                    }
                }
            })
            .collect();
        let assembly = AssemblyPlan {
            sources: vec![SourceDoc {
                path: input.path.clone(),
                password: input.password.clone(),
            }],
            pages,
        };
        let total = u32::try_from(steps.len()).unwrap_or(u32::MAX);
        let label = if o.mode == InsertMode::Duplicate {
            "pages duplicated"
        } else {
            self.label()
        };
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(ctx, &self.deps, input, label, total, &mut |tmp| {
            self.deps.writer.assemble(&assembly, ctx.temp_dir, tmp)
        })?;
        progress.advance(plan.total_units);
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
    fn validates_selection_and_count() {
        let tool = InsertPagesTool::new(ToolDeps::unavailable());
        assert!(
            tool.validate(
                &[doc(3)],
                &ToolOptions::InsertPages(InsertPagesOptions::default())
            )
            .is_ok()
        );
        let bad_page = ToolOptions::InsertPages(InsertPagesOptions {
            pages: PageRange::parse("7").unwrap(),
            ..InsertPagesOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(3)], &bad_page).unwrap_err().code,
            domain::ErrorCode::RangeOutOfBounds
        );
        let bad_count = ToolOptions::InsertPages(InsertPagesOptions {
            count: 0,
            ..InsertPagesOptions::default()
        });
        assert!(tool.validate(&[doc(3)], &bad_count).is_err());
    }
}
