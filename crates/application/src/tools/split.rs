//! Splits that depend on the content of the document: in half, by size, by bookmarks, by text.

use domain::planners::{PageSpan, bookmark_groups, group_by_text, half_plan, halve, pack_by_size};
use domain::tools::{
    SplitByBookmarksOptions, SplitBySizeOptions, SplitByTextOptions, SplitInHalfOptions, ToolGroup,
};
use domain::{DomainError, ErrorCode, PageIndex, ToolId, ToolOptions};
use std::cell::Cell;
use std::collections::VecDeque;

use super::common::{
    Progress, Scratch, ToolDeps, first_input, page_count, pages_label, publish_pdf, wrong_options,
};
use crate::error::AppError;
use crate::ports::{AssemblyPlan, LogLevel, PdfEdit, PlannedPage, SourceDoc};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};
use domain::tools::common::RightAngle;

fn single_plan(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
    let input = first_input(inputs)?;
    Ok(Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: u64::from(page_count(input)),
    })
}

fn pages_of(span: PageSpan) -> Vec<PageIndex> {
    span.pages().map(PageIndex::new).collect()
}

/// Split in half (Appendix A row 9).
pub struct SplitInHalfTool {
    deps: ToolDeps,
}

impl SplitInHalfTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn half(opts: &ToolOptions) -> Result<&SplitInHalfOptions, AppError> {
    match opts {
        ToolOptions::SplitInHalf(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for SplitInHalfTool {
    fn id(&self) -> ToolId {
        ToolId::SplitInHalf
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "halves"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(SplitInHalfOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        half(opts)?;
        single_plan(self.id(), inputs, opts)
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let o = half(&plan.options)?;
        let steps = half_plan(
            page_count(input),
            o.direction.into(),
            o.skip_first,
            o.skip_last,
            o.right_to_left,
        );
        let pages: Vec<PlannedPage> = steps
            .iter()
            .map(|s| PlannedPage::Source {
                source: 0,
                page: PageIndex::new(s.page),
                rotate: RightAngle::Deg0,
            })
            .collect();
        let crops: Vec<_> = steps
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.crop
                    .map(|c| (PageIndex::new(u32::try_from(i).unwrap_or(u32::MAX)), c))
            })
            .collect();
        let assembly = AssemblyPlan {
            sources: vec![SourceDoc {
                path: input.path.clone(),
                password: input.password.clone(),
            }],
            pages,
        };
        let out_pages = u32::try_from(steps.len()).unwrap_or(u32::MAX);
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(
            ctx,
            &self.deps,
            input,
            self.label(),
            out_pages,
            &mut |tmp| {
                // Duplicate every page, then crop each copy to its half.
                let scratch = Scratch::new(ctx, "pdf");
                self.deps
                    .writer
                    .assemble(&assembly, ctx.temp_dir, scratch.path())?;
                self.deps
                    .editor
                    .apply(scratch.path(), &PdfEdit::CropFractions(crops.clone()), tmp)
            },
        )?;
        progress.advance(plan.total_units);
        Ok(vec![out])
    }
}

/// Split by size (Appendix A row 10).
pub struct SplitBySizeTool {
    deps: ToolDeps,
}

impl SplitBySizeTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn by_size(opts: &ToolOptions) -> Result<&SplitBySizeOptions, AppError> {
    match opts {
        ToolOptions::SplitBySize(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

/// Documents up to this many pages get a measured size per page; larger ones use the average.
const MEASURE_PAGES_LIMIT: u32 = 300;

impl SplitBySizeTool {
    /// Estimated size of each page: measured by writing it alone for small documents (this
    /// overestimates shared resources, which is the safe direction), the average for large ones.
    fn estimates(&self, ctx: &JobContext<'_>, input: &InputDoc) -> Result<Vec<u64>, AppError> {
        let count = page_count(input);
        if count > MEASURE_PAGES_LIMIT {
            let average = ctx.files.len(&input.path)? / u64::from(count.max(1));
            return Ok(vec![average; count as usize]);
        }
        (0..count)
            .map(|page| {
                ctx.cancel.check()?;
                let scratch = Scratch::new(ctx, "pdf");
                self.deps.writer.copy_pages(
                    &input.path,
                    input.password.as_ref(),
                    &[PageIndex::new(page)],
                    scratch.path(),
                )?;
                ctx.files.len(scratch.path())
            })
            .collect()
    }
}

impl Tool for SplitBySizeTool {
    fn id(&self) -> ToolId {
        ToolId::SplitBySize
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
        schemars::schema_for!(SplitBySizeOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        by_size(opts)?.limit()?;
        single_plan(self.id(), inputs, opts)
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let limit = by_size(&plan.options)?.limit()?.get();
        let estimates = self.estimates(ctx, input)?;
        let mut queue: VecDeque<PageSpan> = pack_by_size(&estimates, limit).into();
        let mut progress = Progress::start(ctx, plan.total_units);
        let mut outputs = Vec::new();

        while let Some(span) = queue.pop_front() {
            let pages = pages_of(span);
            let too_big = Cell::new(false);
            let dest = ctx.output_path(input, &pages_label(&pages), "pdf", |_| {})?;
            let published = ctx.files.publish(
                &dest,
                &mut |tmp| {
                    self.deps
                        .writer
                        .copy_pages(&input.path, input.password.as_ref(), &pages, tmp)
                },
                &|tmp| {
                    self.deps.verifier.verify(tmp, None, Some(span.len()))?;
                    if span.len() > 1 && ctx.files.len(tmp)? > limit {
                        too_big.set(true);
                        return Err(AppError::new(
                            ErrorCode::OutputVerifyFailed,
                            "over the size limit",
                        ));
                    }
                    Ok(())
                },
            );
            match published {
                Ok(path) => {
                    if span.len() == 1 && ctx.files.len(&path)? > limit {
                        ctx.logger
                            .log(LogLevel::Warn, "split_by_size.page_exceeds_limit");
                    }
                    let bytes = ctx.files.len(&path)?;
                    outputs.push(OutputArtifact {
                        path,
                        pages: Some(span.len()),
                        bytes,
                    });
                    progress.advance(u64::from(span.len()));
                }
                // The estimate was too optimistic: shrink this run and try its halves.
                Err(_) if too_big.get() => {
                    if let Some((first, second)) = halve(span) {
                        queue.push_front(second);
                        queue.push_front(first);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(outputs)
    }
}

/// Split by bookmarks (Appendix A row 8).
pub struct SplitByBookmarksTool {
    deps: ToolDeps,
}

impl SplitByBookmarksTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn by_bookmarks(opts: &ToolOptions) -> Result<&SplitByBookmarksOptions, AppError> {
    match opts {
        ToolOptions::SplitByBookmarks(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for SplitByBookmarksTool {
    fn id(&self) -> ToolId {
        ToolId::SplitByBookmarks
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
        schemars::schema_for!(SplitByBookmarksOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        by_bookmarks(opts)?.validate()?;
        single_plan(self.id(), inputs, opts)
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = by_bookmarks(&plan.options)?;
        let count = page_count(input);
        let items: Vec<(String, u32, u32)> = self
            .deps
            .outlines
            .outline(&input.path, input.password.as_ref())?
            .into_iter()
            .filter_map(|i| i.page.map(|p| (i.title, p.get(), i.level)))
            .collect();
        if !items.iter().any(|(_, _, level)| *level == opts.level) {
            return Err(DomainError::option(
                "level",
                format!("this file has no bookmarks at level {}", opts.level),
            )
            .into());
        }
        let groups = bookmark_groups(&items, opts.level, count, "front matter");
        let mut progress = Progress::start(ctx, plan.total_units);
        let mut outputs = Vec::with_capacity(groups.len());
        for (i, (span, title)) in groups.iter().enumerate() {
            let pages = pages_of(*span);
            let label = if opts.name_from_bookmark && !title.trim().is_empty() {
                title.trim().to_owned()
            } else {
                format!("part {}", i + 1)
            };
            outputs.push(publish_pdf(
                ctx,
                &self.deps,
                input,
                &label,
                span.len(),
                &mut |tmp| {
                    self.deps
                        .writer
                        .copy_pages(&input.path, input.password.as_ref(), &pages, tmp)
                },
            )?);
            progress.advance(u64::from(span.len()));
        }
        Ok(outputs)
    }
}

/// Split by text (Appendix A row 11).
pub struct SplitByTextTool {
    deps: ToolDeps,
}

impl SplitByTextTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn by_text(opts: &ToolOptions) -> Result<&SplitByTextOptions, AppError> {
    match opts {
        ToolOptions::SplitByText(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for SplitByTextTool {
    fn id(&self) -> ToolId {
        ToolId::SplitByText
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
        schemars::schema_for!(SplitByTextOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        by_text(opts)?.region()?;
        single_plan(self.id(), inputs, opts)
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = by_text(&plan.options)?;
        let region = opts.region()?;
        let count = page_count(input);

        let mut texts = Vec::with_capacity(count as usize);
        for page in 0..count {
            ctx.cancel.check()?;
            texts.push(self.deps.text.text_in_region(
                &input.path,
                input.password.as_ref(),
                PageIndex::new(page),
                &region,
            )?);
        }
        let groups = group_by_text(&texts, opts.starts_with.as_deref());
        let mut progress = Progress::start(ctx, plan.total_units);
        let mut outputs = Vec::with_capacity(groups.len());
        for (i, (span, text)) in groups.iter().enumerate() {
            let pages = pages_of(*span);
            let label = if text.is_empty() {
                format!("part {}", i + 1)
            } else {
                text.clone()
            };
            outputs.push(publish_pdf(
                ctx,
                &self.deps,
                input,
                &label,
                span.len(),
                &mut |tmp| {
                    self.deps
                        .writer
                        .copy_pages(&input.path, input.password.as_ref(), &pages, tmp)
                },
            )?);
            progress.advance(u64::from(span.len()));
        }
        Ok(outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::common::Region;

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn text_split_needs_a_region() {
        let tool = SplitByTextTool::new(ToolDeps::unavailable());
        let none = ToolOptions::SplitByText(SplitByTextOptions::default());
        assert_eq!(
            tool.validate(&[doc(3)], &none).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let some = ToolOptions::SplitByText(SplitByTextOptions {
            region: Some(Region {
                x: 0.1,
                y: 0.1,
                width: 0.3,
                height: 0.1,
            }),
            starts_with: None,
        });
        assert!(tool.validate(&[doc(3)], &some).is_ok());
    }

    #[test]
    fn size_and_bookmark_options_are_validated() {
        let size = SplitBySizeTool::new(ToolDeps::unavailable());
        let bad = ToolOptions::SplitBySize(SplitBySizeOptions { max_size_mb: 0 });
        assert!(size.validate(&[doc(3)], &bad).is_err());
        let bm = SplitByBookmarksTool::new(ToolDeps::unavailable());
        let level0 = ToolOptions::SplitByBookmarks(SplitByBookmarksOptions {
            level: 0,
            name_from_bookmark: true,
        });
        assert!(bm.validate(&[doc(3)], &level0).is_err());
    }

    #[test]
    fn half_validates_and_counts_input_pages() {
        let tool = SplitInHalfTool::new(ToolDeps::unavailable());
        let ok = ToolOptions::SplitInHalf(SplitInHalfOptions::default());
        assert_eq!(tool.validate(&[doc(4)], &ok).unwrap().total_units, 4);
    }
}
