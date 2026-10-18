//! Tools that build one PDF from several: merge, alternate and mix, organize.

use domain::planners::interleave;
use domain::tools::combine::{FormHandling, OrganizedPage};
use domain::tools::common::RightAngle;
use domain::tools::{AlternateMixOptions, MergeOptions, OrganizeOptions, ToolGroup};
use domain::{DomainError, PageIndex, ToolId, ToolOptions};
use std::path::PathBuf;

use super::common::{
    Progress, Scratch, ToolDeps, first_input, page_count, publish_pdf, wrong_options,
};
use crate::error::AppError;
use crate::ports::{
    AssemblyPlan, OutlineEntry, PageSize, PdfEdit, PlannedPage, SourceDoc, TocLine,
};
use crate::runtime::Password;
use crate::tool::{InputDoc, InputKind, InputMode, JobContext, OutputArtifact, Plan, Tool};

/// US Letter, used when no page size can be read.
const FALLBACK_SIZE: PageSize = PageSize {
    width_pt: 612.0,
    height_pt: 792.0,
};

fn file_stem(input: &InputDoc) -> String {
    input.stem()
}

/// Publishes a document made by `assemble` and then `edits` applied one after another. Scratch
/// files carry the intermediate results.
fn assemble_and_edit(
    deps: &ToolDeps,
    ctx: &JobContext<'_>,
    assembly: &AssemblyPlan,
    edits: &[PdfEdit],
    tmp: &std::path::Path,
) -> Result<(), AppError> {
    if edits.is_empty() {
        return deps.writer.assemble(assembly, ctx.temp_dir, tmp);
    }
    let mut current = Scratch::new(ctx, "pdf");
    deps.writer
        .assemble(assembly, ctx.temp_dir, current.path())?;
    for (i, edit) in edits.iter().enumerate() {
        if i + 1 == edits.len() {
            deps.editor.apply(current.path(), edit, tmp)?;
        } else {
            let next = Scratch::new(ctx, "pdf");
            deps.editor.apply(current.path(), edit, next.path())?;
            current = next;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------------------------

/// Merge PDFs and images (Appendix A row 1).
pub struct MergeTool {
    deps: ToolDeps,
}

impl MergeTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn merge(opts: &ToolOptions) -> Result<&MergeOptions, AppError> {
    match opts {
        ToolOptions::Merge(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

/// One input made ready for assembly: converted from an image or flattened when needed.
struct Prepared<'a> {
    path: PathBuf,
    password: Option<Password>,
    pages: u32,
    // Held so the converted or flattened copy is removed when the merge ends.
    _scratch: Option<Scratch<'a>>,
}

impl MergeTool {
    fn prepare<'a>(
        &self,
        ctx: &'a JobContext<'a>,
        input: &InputDoc,
        forms: FormHandling,
    ) -> Result<Prepared<'a>, AppError> {
        ctx.cancel.check()?;
        let is_image = input.page_count.is_none();
        if is_image {
            let scratch = Scratch::new(ctx, "pdf");
            self.deps.images.image_to_pdf(&input.path, scratch.path())?;
            return Ok(Prepared {
                path: scratch.path().to_path_buf(),
                password: None,
                pages: 1,
                _scratch: Some(scratch),
            });
        }
        if forms == FormHandling::Flatten {
            let scratch = Scratch::new(ctx, "pdf");
            self.deps.flattener.flatten_forms(
                &input.path,
                input.password.as_ref(),
                scratch.path(),
            )?;
            return Ok(Prepared {
                path: scratch.path().to_path_buf(),
                password: None,
                pages: page_count(input),
                _scratch: Some(scratch),
            });
        }
        Ok(Prepared {
            path: input.path.clone(),
            password: input.password.clone(),
            pages: page_count(input),
            _scratch: None,
        })
    }
}

impl Tool for MergeTool {
    fn id(&self) -> ToolId {
        ToolId::Merge
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "merged"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf, InputKind::Image]
    }
    fn input_mode(&self) -> InputMode {
        InputMode::AllTogether
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(MergeOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let o = merge(opts)?;
        o.validate(inputs.len())?;
        first_input(inputs)?;
        let mut total = 0u64;
        for (i, input) in inputs.iter().enumerate() {
            // Images count as one page; their per-file range is checked against that.
            let pages = input.page_count.unwrap_or(1);
            total += o.pages_for(i, pages)?.len() as u64;
        }
        Ok(Plan {
            tool: ToolId::Merge,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: total,
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let o = merge(&plan.options)?;
        let first = first_input(&plan.inputs)?;
        let mut progress = Progress::start(ctx, plan.total_units);

        let prepared: Vec<Prepared<'_>> = plan
            .inputs
            .iter()
            .map(|input| self.prepare(ctx, input, o.forms))
            .collect::<Result<_, _>>()?;

        let mut sources: Vec<SourceDoc> = prepared
            .iter()
            .map(|p| SourceDoc {
                path: p.path.clone(),
                password: p.password.clone(),
            })
            .collect();
        let mut selections: Vec<Vec<PageIndex>> = Vec::with_capacity(prepared.len());
        for (i, p) in prepared.iter().enumerate() {
            selections.push(o.pages_for(i, p.pages)?);
        }
        if selections.iter().all(Vec::is_empty) {
            return Err(DomainError::option("per_file", "no pages are selected").into());
        }

        let first_size = self
            .deps
            .reader
            .page_sizes(&prepared[0].path, prepared[0].password.as_ref())?
            .get(selections[0].first().map_or(0, |p| p.get() as usize))
            .copied()
            .unwrap_or(FALLBACK_SIZE);

        // Table of contents pages come first and shift every file's start page.
        let toc_pages = if o.table_of_contents {
            self.deps.toc.page_count(plan.inputs.len())
        } else {
            0
        };
        let mut pages: Vec<PlannedPage> = Vec::new();
        let toc_scratch = Scratch::new(ctx, "pdf");
        let mut starts: Vec<u32> = Vec::new();
        let mut offset = toc_pages;
        for selection in &selections {
            starts.push(offset);
            offset += u32::try_from(selection.len()).unwrap_or(u32::MAX);
        }
        if o.table_of_contents {
            let lines: Vec<TocLine> = plan
                .inputs
                .iter()
                .zip(&starts)
                .map(|(input, start)| TocLine {
                    title: file_stem(input),
                    page_number: start + 1,
                })
                .collect();
            self.deps.toc.build(
                &lines,
                first_size.width_pt,
                first_size.height_pt,
                toc_scratch.path(),
            )?;
            let toc_index = sources.len();
            sources.push(SourceDoc {
                path: toc_scratch.path().to_path_buf(),
                password: None,
            });
            for page in 0..toc_pages {
                pages.push(PlannedPage::Source {
                    source: toc_index,
                    page: PageIndex::new(page),
                    rotate: RightAngle::Deg0,
                });
            }
        }
        for (i, selection) in selections.iter().enumerate() {
            let rotate = o.file(i).rotation;
            for page in selection {
                pages.push(PlannedPage::Source {
                    source: i,
                    page: *page,
                    rotate,
                });
            }
        }
        let total_pages = u32::try_from(pages.len()).unwrap_or(u32::MAX);

        let mut edits: Vec<PdfEdit> = Vec::new();
        if o.normalize_page_size {
            edits.push(PdfEdit::ScalePagesTo {
                width_pt: first_size.width_pt,
                height_pt: first_size.height_pt,
            });
        }
        if o.bookmark_per_file {
            let mut entries: Vec<OutlineEntry> = plan
                .inputs
                .iter()
                .zip(&starts)
                .zip(&selections)
                .filter(|(_, selection)| !selection.is_empty())
                .map(|((input, start), _)| OutlineEntry {
                    title: file_stem(input),
                    page: PageIndex::new(*start),
                    level: 0,
                })
                .collect();
            if o.table_of_contents {
                entries.insert(
                    0,
                    OutlineEntry {
                        title: "Contents".to_owned(),
                        page: PageIndex::new(0),
                        level: 0,
                    },
                );
            }
            edits.push(PdfEdit::SetOutline(entries));
        }

        let assembly = AssemblyPlan { sources, pages };
        let out = publish_pdf(
            ctx,
            &self.deps,
            first,
            self.label(),
            total_pages,
            &mut |tmp| assemble_and_edit(&self.deps, ctx, &assembly, &edits, tmp),
        )?;
        progress.advance(plan.total_units);
        Ok(vec![out])
    }
}

// ---------------------------------------------------------------------------------------------
// Alternate and mix
// ---------------------------------------------------------------------------------------------

/// Alternate and mix pages of several files (Appendix A row 2).
pub struct AlternateMixTool {
    deps: ToolDeps,
}

impl AlternateMixTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn mix(opts: &ToolOptions) -> Result<&AlternateMixOptions, AppError> {
    match opts {
        ToolOptions::AlternateMix(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for AlternateMixTool {
    fn id(&self) -> ToolId {
        ToolId::AlternateMix
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "mixed"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn input_mode(&self) -> InputMode {
        InputMode::AllTogether
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(AlternateMixOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        mix(opts)?.validate(inputs.len())?;
        Ok(Plan {
            tool: ToolId::AlternateMix,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: inputs.iter().map(|i| u64::from(page_count(i))).sum(),
        })
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let o = mix(&plan.options)?;
        let counts: Vec<u32> = plan.inputs.iter().map(page_count).collect();
        let order = interleave(&counts, o.pages_per_turn, &o.reverse_flags(counts.len()));
        let pages: Vec<PlannedPage> = order
            .iter()
            .map(|(doc, page)| PlannedPage::Source {
                source: *doc,
                page: PageIndex::new(*page),
                rotate: RightAngle::Deg0,
            })
            .collect();
        let assembly = AssemblyPlan {
            sources: plan
                .inputs
                .iter()
                .map(|i| SourceDoc {
                    path: i.path.clone(),
                    password: i.password.clone(),
                })
                .collect(),
            pages,
        };
        let total = u32::try_from(order.len()).unwrap_or(u32::MAX);
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(
            ctx,
            &self.deps,
            first_input(&plan.inputs)?,
            self.label(),
            total,
            &mut |tmp| self.deps.writer.assemble(&assembly, ctx.temp_dir, tmp),
        )?;
        progress.advance(plan.total_units);
        Ok(vec![out])
    }
}

// ---------------------------------------------------------------------------------------------
// Organize
// ---------------------------------------------------------------------------------------------

/// Organize pages visually (Appendix A row 3). The organizer sends the final page list.
pub struct OrganizeTool {
    deps: ToolDeps,
}

impl OrganizeTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn organize(opts: &ToolOptions) -> Result<&OrganizeOptions, AppError> {
    match opts {
        ToolOptions::Organize(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl OrganizeTool {
    /// The pages of the result: the list from the organizer, or the first file unchanged.
    fn resolved(inputs: &[InputDoc], o: &OrganizeOptions) -> Vec<OrganizedPage> {
        if !o.pages.is_empty() {
            return o.pages.clone();
        }
        let count = inputs.first().map_or(0, page_count);
        (0..count)
            .map(|page| OrganizedPage::Page {
                source: 0,
                page,
                rotate: RightAngle::Deg0,
            })
            .collect()
    }
}

impl Tool for OrganizeTool {
    fn id(&self) -> ToolId {
        ToolId::Organize
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "organized"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn input_mode(&self) -> InputMode {
        InputMode::AllTogether
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(OrganizeOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let o = organize(opts)?;
        first_input(inputs)?;
        let counts: Vec<u32> = inputs.iter().map(page_count).collect();
        o.validate(&counts)?;
        let total = Self::resolved(inputs, o).len() as u64;
        if total == 0 {
            return Err(DomainError::option("pages", "the result would have no pages").into());
        }
        Ok(Plan {
            tool: ToolId::Organize,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: total,
        })
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let o = organize(&plan.options)?;
        let list = Self::resolved(&plan.inputs, o);
        let pages: Vec<PlannedPage> = list
            .iter()
            .map(|entry| match entry {
                OrganizedPage::Page {
                    source,
                    page,
                    rotate,
                } => PlannedPage::Source {
                    source: *source as usize,
                    page: PageIndex::new(*page),
                    rotate: *rotate,
                },
                OrganizedPage::Blank {
                    width_pt,
                    height_pt,
                } => PlannedPage::Blank {
                    width_pt: *width_pt,
                    height_pt: *height_pt,
                },
            })
            .collect();
        let assembly = AssemblyPlan {
            sources: plan
                .inputs
                .iter()
                .map(|i| SourceDoc {
                    path: i.path.clone(),
                    password: i.password.clone(),
                })
                .collect(),
            pages,
        };
        let total = u32::try_from(list.len()).unwrap_or(u32::MAX);
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(
            ctx,
            &self.deps,
            first_input(&plan.inputs)?,
            self.label(),
            total,
            &mut |tmp| self.deps.writer.assemble(&assembly, ctx.temp_dir, tmp),
        )?;
        progress.advance(plan.total_units);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::PageRange;
    use domain::tools::combine::MergeFileOptions;

    fn pdf(name: &str, pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new(name)
        }
    }

    fn image(name: &str) -> InputDoc {
        InputDoc::new(name)
    }

    #[test]
    fn merge_counts_selected_pages_and_treats_images_as_one_page() {
        let tool = MergeTool::new(ToolDeps::unavailable());
        let opts = ToolOptions::Merge(MergeOptions {
            per_file: vec![MergeFileOptions {
                pages: PageRange::parse("1-2").unwrap(),
                ..Default::default()
            }],
            ..MergeOptions::default()
        });
        let plan = tool
            .validate(
                &[pdf("C:\\a.pdf", 5), pdf("C:\\b.pdf", 3), image("C:\\c.png")],
                &opts,
            )
            .unwrap();
        assert_eq!(plan.total_units, 2 + 3 + 1);
    }

    #[test]
    fn merge_rejects_bad_ranges_and_extra_settings() {
        let tool = MergeTool::new(ToolDeps::unavailable());
        let bad = ToolOptions::Merge(MergeOptions {
            per_file: vec![MergeFileOptions {
                pages: PageRange::parse("9").unwrap(),
                ..Default::default()
            }],
            ..MergeOptions::default()
        });
        assert_eq!(
            tool.validate(&[pdf("C:\\a.pdf", 3)], &bad)
                .unwrap_err()
                .code,
            domain::ErrorCode::RangeOutOfBounds
        );
        let extra = ToolOptions::Merge(MergeOptions {
            per_file: vec![MergeFileOptions::default(); 3],
            ..MergeOptions::default()
        });
        assert!(tool.validate(&[pdf("C:\\a.pdf", 3)], &extra).is_err());
        assert!(
            tool.validate(&[], &ToolOptions::Merge(MergeOptions::default()))
                .is_err()
        );
    }

    #[test]
    fn alternate_mix_needs_two_files_and_counts_all_pages() {
        let tool = AlternateMixTool::new(ToolDeps::unavailable());
        let opts = ToolOptions::AlternateMix(AlternateMixOptions::default());
        assert!(tool.validate(&[pdf("C:\\a.pdf", 3)], &opts).is_err());
        let plan = tool
            .validate(&[pdf("C:\\a.pdf", 3), pdf("C:\\b.pdf", 4)], &opts)
            .unwrap();
        assert_eq!(plan.total_units, 7);
    }

    #[test]
    fn organize_defaults_to_the_first_file_and_checks_entries() {
        let tool = OrganizeTool::new(ToolDeps::unavailable());
        let empty = ToolOptions::Organize(OrganizeOptions::default());
        assert_eq!(
            tool.validate(&[pdf("C:\\a.pdf", 4)], &empty)
                .unwrap()
                .total_units,
            4
        );
        let bad = ToolOptions::Organize(OrganizeOptions {
            pages: vec![OrganizedPage::Page {
                source: 0,
                page: 4,
                rotate: RightAngle::Deg0,
            }],
        });
        assert_eq!(
            tool.validate(&[pdf("C:\\a.pdf", 4)], &bad)
                .unwrap_err()
                .code,
            domain::ErrorCode::RangeOutOfBounds
        );
        let zero = ToolOptions::Organize(OrganizeOptions::default());
        assert!(tool.validate(&[pdf("C:\\a.pdf", 0)], &zero).is_err());
    }
}
