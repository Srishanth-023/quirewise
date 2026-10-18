//! Tools that write one or more files, each holding a chosen set of pages from one input:
//! extract, remove, split by ranges, split every N pages.

use domain::planners::every_n;
use domain::tools::{
    ExtractPagesOptions, RemovePagesOptions, SplitEveryNOptions, SplitRangesOptions, ToolGroup,
};
use domain::{PageIndex, ToolId, ToolOptions};

use super::common::{
    Progress, ToolDeps, first_input, page_count, pages_label, publish_pdf, wrong_options,
};
use crate::error::AppError;
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Writes each group of pages as its own PDF, named by `label_for`.
fn write_groups(
    deps: &ToolDeps,
    ctx: &JobContext<'_>,
    plan: &Plan,
    groups: &[Vec<PageIndex>],
    label_for: &dyn Fn(usize, &[PageIndex]) -> String,
) -> Result<Vec<OutputArtifact>, AppError> {
    let input = first_input(&plan.inputs)?;
    let mut progress = Progress::start(ctx, plan.total_units);
    let mut outputs = Vec::with_capacity(groups.len());
    for (i, pages) in groups.iter().enumerate() {
        let count = u32::try_from(pages.len()).unwrap_or(u32::MAX);
        let label = label_for(i, pages);
        outputs.push(publish_pdf(ctx, deps, input, &label, count, &mut |tmp| {
            deps.writer
                .copy_pages(&input.path, input.password.as_ref(), pages, tmp)
        })?);
        progress.advance(u64::from(count));
    }
    Ok(outputs)
}

fn single_plan(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
    let input = first_input(inputs)?;
    Ok(Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: u64::from(page_count(input)),
    })
}

/// Extract pages (Appendix A row 6).
pub struct ExtractPagesTool {
    deps: ToolDeps,
}

impl ExtractPagesTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn extract(opts: &ToolOptions) -> Result<&ExtractPagesOptions, AppError> {
    match opts {
        ToolOptions::ExtractPages(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for ExtractPagesTool {
    fn id(&self) -> ToolId {
        ToolId::ExtractPages
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "extracted"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ExtractPagesOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        extract(opts)?.groups(page_count(input))?;
        single_plan(self.id(), inputs, opts)
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = extract(&plan.options)?;
        let groups = opts.groups(page_count(input))?;
        let one_file = opts.one_file;
        write_groups(&self.deps, ctx, &plan, &groups, &|_, pages| {
            if one_file {
                self.label().to_owned()
            } else {
                pages_label(pages)
            }
        })
    }
}

/// Remove pages (Appendix A row 7).
pub struct RemovePagesTool {
    deps: ToolDeps,
}

impl RemovePagesTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn remove(opts: &ToolOptions) -> Result<&RemovePagesOptions, AppError> {
    match opts {
        ToolOptions::RemovePages(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for RemovePagesTool {
    fn id(&self) -> ToolId {
        ToolId::RemovePages
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "pages removed"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(RemovePagesOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        remove(opts)?.kept_pages(page_count(input))?;
        single_plan(self.id(), inputs, opts)
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let kept = remove(&plan.options)?.kept_pages(page_count(input))?;
        write_groups(&self.deps, ctx, &plan, &[kept], &|_, _| {
            self.label().to_owned()
        })
    }
}

/// Split by ranges (Appendix A row 4).
pub struct SplitRangesTool {
    deps: ToolDeps,
}

impl SplitRangesTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn ranges(opts: &ToolOptions) -> Result<&SplitRangesOptions, AppError> {
    match opts {
        ToolOptions::SplitRanges(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for SplitRangesTool {
    fn id(&self) -> ToolId {
        ToolId::SplitRanges
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "split"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(SplitRangesOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        ranges(opts)?.groups(page_count(input))?;
        single_plan(self.id(), inputs, opts)
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = ranges(&plan.options)?;
        let groups = opts.groups(page_count(input))?;
        let merged = opts.merge_into_one;
        write_groups(&self.deps, ctx, &plan, &groups, &|_, pages| {
            if merged {
                "selected pages".to_owned()
            } else {
                pages_label(pages)
            }
        })
    }
}

/// Split every N pages (Appendix A row 5).
pub struct SplitEveryNTool {
    deps: ToolDeps,
}

impl SplitEveryNTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn every(opts: &ToolOptions) -> Result<&SplitEveryNOptions, AppError> {
    match opts {
        ToolOptions::SplitEveryN(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for SplitEveryNTool {
    fn id(&self) -> ToolId {
        ToolId::SplitEveryN
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "split"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(SplitEveryNOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        every(opts)?.validate()?;
        single_plan(self.id(), inputs, opts)
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let n = every(&plan.options)?.n;
        let groups: Vec<Vec<PageIndex>> = every_n(page_count(input), n)
            .into_iter()
            .map(|span| span.pages().map(PageIndex::new).collect())
            .collect();
        write_groups(&self.deps, ctx, &plan, &groups, &|_, pages| {
            pages_label(pages)
        })
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

    fn deps() -> ToolDeps {
        ToolDeps::unavailable()
    }

    #[test]
    fn extract_validates_ranges_and_counts_units() {
        let tool = ExtractPagesTool::new(deps());
        let ok = ToolOptions::ExtractPages(ExtractPagesOptions::default());
        assert_eq!(tool.validate(&[doc(7)], &ok).unwrap().total_units, 7);
        let bad = ToolOptions::ExtractPages(ExtractPagesOptions {
            pages: PageRange::parse("8").unwrap(),
            one_file: true,
        });
        assert_eq!(
            tool.validate(&[doc(7)], &bad).unwrap_err().code,
            domain::ErrorCode::RangeOutOfBounds
        );
    }

    #[test]
    fn remove_refuses_to_empty_the_document() {
        let tool = RemovePagesTool::new(deps());
        let all = ToolOptions::RemovePages(RemovePagesOptions {
            pages: PageRange::all(),
        });
        assert_eq!(
            tool.validate(&[doc(3)], &all).unwrap_err().code,
            domain::ErrorCode::OptionInvalid
        );
        let one = ToolOptions::RemovePages(RemovePagesOptions::default());
        assert!(tool.validate(&[doc(3)], &one).is_ok());
    }

    #[test]
    fn split_tools_validate_their_options() {
        let n0 = ToolOptions::SplitEveryN(SplitEveryNOptions { n: 0 });
        assert!(
            SplitEveryNTool::new(deps())
                .validate(&[doc(3)], &n0)
                .is_err()
        );
        let overlap = ToolOptions::SplitRanges(SplitRangesOptions {
            ranges: PageRange::parse("1-3,2").unwrap(),
            merge_into_one: false,
        });
        assert!(
            SplitRangesTool::new(deps())
                .validate(&[doc(5)], &overlap)
                .is_err()
        );
    }

    #[test]
    fn options_for_another_tool_are_refused() {
        let wrong = ToolOptions::Rotate(Default::default());
        assert_eq!(
            ExtractPagesTool::new(deps())
                .validate(&[doc(3)], &wrong)
                .unwrap_err()
                .code,
            domain::ErrorCode::Internal
        );
    }
}
