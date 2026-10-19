//! The structure tools: Bookmarks editor (row 49), Rename by content (row 50), Attachments
//! (row 65) and Page labels (row 66).

use domain::PageIndex;
use domain::naming::NameContext;
use domain::tools::ToolGroup;
use domain::tools::structure::{
    AttachmentAction, AttachmentsOptions, BookmarksOptions, PageLabelsOptions,
    RenameByContentOptions, check_bookmarks, detect_headings,
};
use domain::{ErrorCode, ToolId, ToolOptions};
use std::path::PathBuf;

use super::common::{Progress, ToolDeps, first_input, page_count, publish_edit, wrong_options};
use crate::error::AppError;
use crate::ports::{OutlineEntry, PdfEdit};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions, units: u64) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: units,
    }
}

/// Edits the bookmarks, or builds them from the headings of the document.
pub struct BookmarksTool {
    deps: ToolDeps,
}

impl BookmarksTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn bookmark_options(opts: &ToolOptions) -> Result<&BookmarksOptions, AppError> {
    match opts {
        ToolOptions::Bookmarks(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for BookmarksTool {
    fn id(&self) -> ToolId {
        ToolId::Bookmarks
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "bookmarked"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(BookmarksOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let options = bookmark_options(opts)?;
        options.validate()?;
        if !options.auto_generate {
            check_bookmarks(&options.bookmarks, page_count(input))?;
        }
        Ok(plan_for(ToolId::Bookmarks, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = bookmark_options(&plan.options)?;
        let count = page_count(input);
        let mut progress = Progress::start(ctx, plan.total_units);
        let entries = if options.auto_generate {
            let lines = self
                .deps
                .headings
                .text_lines(&input.path, input.password.as_ref())?;
            ctx.cancel.check()?;
            let found = detect_headings(&lines, count, options.auto_max_level);
            if found.is_empty() {
                return Err(AppError::new(
                    ErrorCode::OptionInvalid,
                    "No headings were found: no text is set larger than the body text. Add the bookmarks by hand, or run OCR first if the document is a scan.",
                ));
            }
            found
        } else {
            options.bookmarks.clone()
        };
        let outline: Vec<OutlineEntry> = check_bookmarks(&entries, count)?
            .into_iter()
            .map(|(title, page, level)| OutlineEntry {
                title,
                page,
                level: level - 1,
            })
            .collect();
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            count,
            &PdfEdit::SetOutline(outline),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Copies a file under a name made from text on one of its pages.
pub struct RenameByContentTool {
    deps: ToolDeps,
}

impl RenameByContentTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn rename_options(opts: &ToolOptions) -> Result<&RenameByContentOptions, AppError> {
    match opts {
        ToolOptions::RenameByContent(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

/// Text read from a page reduced to one line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Tool for RenameByContentTool {
    fn id(&self) -> ToolId {
        ToolId::RenameByContent
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Organize
    }
    fn label(&self) -> &'static str {
        "renamed"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(RenameByContentOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let options = rename_options(opts)?;
        options.checked()?;
        let count = page_count(input);
        if options.page > count {
            return Err(domain::DomainError::RangeOutOfBounds {
                detail: format!("Page {} does not exist.", options.page),
                valid: format!("1-{count}"),
            }
            .into());
        }
        Ok(plan_for(ToolId::RenameByContent, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = rename_options(&plan.options)?;
        let (region, pattern) = options.checked()?;
        let count = page_count(input);
        let mut progress = Progress::start(ctx, plan.total_units);

        let text = self.deps.text.text_in_region(
            &input.path,
            input.password.as_ref(),
            PageIndex::new(options.page - 1),
            &region,
        )?;
        let text = one_line(&text);
        if text.is_empty() {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                format!(
                    "No text was found in the box on page {}. Draw the box around the text, or run OCR first if the page is a scan.",
                    options.page
                ),
            ));
        }
        let stem = input.stem();
        let name = NameContext {
            filename: &stem,
            tool: self.label(),
            now: ctx.clock.now(),
            sequence: None,
            page: Some(options.page),
            pages: Some(count),
            bookmark: None,
            text: Some(&text),
        };
        let dest = ctx
            .naming
            .output_path(ctx.output_dir, &pattern, &name, "pdf")?;
        let password = input.password.clone();
        let published = ctx.files.publish(
            &dest,
            &mut |tmp| ctx.files.copy_file(&input.path, tmp),
            &|tmp| {
                self.deps
                    .verifier
                    .verify(tmp, password.as_ref(), Some(count))
            },
        )?;
        let bytes = ctx.files.len(&published)?;
        progress.advance(1);
        Ok(vec![OutputArtifact {
            path: published,
            pages: Some(count),
            bytes,
        }])
    }
}

/// Lists, adds, extracts and removes attached files.
pub struct AttachmentsTool {
    deps: ToolDeps,
}

impl AttachmentsTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn attachment_options(opts: &ToolOptions) -> Result<&AttachmentsOptions, AppError> {
    match opts {
        ToolOptions::Attachments(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

/// A file extension safe to put on an extracted attachment.
fn safe_extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| {
            !ext.is_empty() && ext.len() <= 10 && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .map_or_else(|| "bin".to_owned(), str::to_ascii_lowercase)
}

fn attachment_stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && ext.len() <= 10 => stem,
        _ => name,
    }
}

impl AttachmentsTool {
    fn write_listing(
        &self,
        ctx: &JobContext<'_>,
        input: &InputDoc,
    ) -> Result<OutputArtifact, AppError> {
        let items = self
            .deps
            .attachments
            .list(&input.path, input.password.as_ref())?;
        let mut text = String::new();
        if items.is_empty() {
            text.push_str("This document has no attachments.\r\n");
        } else {
            text.push_str(&format!("{} attachment(s)\r\n\r\n", items.len()));
            for item in &items {
                let size = item
                    .size
                    .map_or_else(|| "unknown size".to_owned(), |s| format!("{s} bytes"));
                text.push_str(&format!("{}\t{size}", item.name));
                if let Some(d) = &item.description {
                    text.push_str(&format!("\t{d}"));
                }
                text.push_str("\r\n");
            }
        }
        let dest = ctx.output_path(input, "attachments", "txt", |_| {})?;
        let path = ctx.files.write_atomic(
            &dest,
            &mut |out| out.write_all(text.as_bytes()),
            &|_| Ok(()),
        )?;
        let bytes = ctx.files.len(&path)?;
        Ok(OutputArtifact {
            path,
            pages: None,
            bytes,
        })
    }

    fn extract(
        &self,
        ctx: &JobContext<'_>,
        input: &InputDoc,
        names: &[String],
        progress: &mut Progress<'_>,
    ) -> Result<Vec<OutputArtifact>, AppError> {
        let files = self
            .deps
            .attachments
            .read(&input.path, input.password.as_ref(), names)?;
        if files.is_empty() {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "This document has no attachments to extract.",
            ));
        }
        let pattern = domain::naming::OutputNamingPattern::parse("{filename} - {text}")?;
        let stem = input.stem();
        let mut outputs = Vec::with_capacity(files.len());
        for file in files {
            ctx.cancel.check()?;
            let name = NameContext {
                filename: &stem,
                tool: "extracted",
                now: ctx.clock.now(),
                sequence: None,
                page: None,
                pages: None,
                bookmark: None,
                text: Some(attachment_stem(&file.name)),
            };
            let dest = ctx.naming.output_path(
                ctx.output_dir,
                &pattern,
                &name,
                &safe_extension(&file.name),
            )?;
            let path =
                ctx.files
                    .write_atomic(&dest, &mut |out| out.write_all(&file.data), &|_| Ok(()))?;
            let bytes = ctx.files.len(&path)?;
            outputs.push(OutputArtifact {
                path,
                pages: None,
                bytes,
            });
            progress.advance(1);
        }
        Ok(outputs)
    }
}

impl Tool for AttachmentsTool {
    fn id(&self) -> ToolId {
        ToolId::Attachments
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "attachments"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(AttachmentsOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        attachment_options(opts)?.validate()?;
        Ok(plan_for(ToolId::Attachments, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = attachment_options(&plan.options)?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let outputs = match options.action {
            AttachmentAction::List => vec![self.write_listing(ctx, input)?],
            AttachmentAction::Extract => {
                let names: Vec<String> =
                    options.name_list().into_iter().map(str::to_owned).collect();
                self.extract(ctx, input, &names, &mut progress)?
            }
            AttachmentAction::Add => {
                let paths: Vec<PathBuf> =
                    options.file_list().into_iter().map(PathBuf::from).collect();
                for path in &paths {
                    if !ctx.files.exists(path) {
                        return Err(AppError::new(
                            ErrorCode::PathDenied,
                            format!(
                                "The file \"{}\" was not found. Check that it still exists.",
                                path.file_name().map_or_else(
                                    || path.to_string_lossy().into_owned(),
                                    |n| n.to_string_lossy().into_owned()
                                )
                            ),
                        ));
                    }
                }
                vec![publish_edit(
                    ctx,
                    &self.deps,
                    input,
                    "attachments added",
                    page_count(input),
                    &PdfEdit::AddAttachments(paths),
                )?]
            }
            AttachmentAction::Delete => {
                let names: Vec<String> =
                    options.name_list().into_iter().map(str::to_owned).collect();
                vec![publish_edit(
                    ctx,
                    &self.deps,
                    input,
                    "attachments removed",
                    page_count(input),
                    &PdfEdit::RemoveAttachments(names),
                )?]
            }
        };
        progress.advance(1);
        Ok(outputs)
    }
}

/// Sets how pages are numbered in a viewer.
pub struct PageLabelsTool {
    deps: ToolDeps,
}

impl PageLabelsTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn label_options(opts: &ToolOptions) -> Result<&PageLabelsOptions, AppError> {
    match opts {
        ToolOptions::PageLabels(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for PageLabelsTool {
    fn id(&self) -> ToolId {
        ToolId::PageLabels
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Edit
    }
    fn label(&self) -> &'static str {
        "page labels"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PageLabelsOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        label_options(opts)?.checked(page_count(input))?;
        Ok(plan_for(ToolId::PageLabels, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = label_options(&plan.options)?;
        let count = page_count(input);
        let ranges = options.checked(count)?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            count,
            &PdfEdit::SetPageLabels(ranges),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::stamp::NumberStyle;
    use domain::tools::common::Region;
    use domain::tools::structure::{BookmarkEntry, LabelRange};

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn bookmarks_are_checked_against_the_page_count_before_running() {
        let tool = BookmarksTool::new(ToolDeps::unavailable());
        let entry = |page| BookmarkEntry {
            title: "A".into(),
            page,
            level: 1,
        };
        let ok = ToolOptions::Bookmarks(BookmarksOptions {
            bookmarks: vec![entry(2)],
            ..BookmarksOptions::default()
        });
        assert!(tool.validate(&[doc(3)], &ok).is_ok());
        let past_the_end = ToolOptions::Bookmarks(BookmarksOptions {
            bookmarks: vec![entry(9)],
            ..BookmarksOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(3)], &past_the_end).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let empty = ToolOptions::defaults_for(ToolId::Bookmarks);
        assert!(tool.validate(&[doc(3)], &empty).is_err());
        let auto = ToolOptions::Bookmarks(BookmarksOptions {
            auto_generate: true,
            ..BookmarksOptions::default()
        });
        assert!(tool.validate(&[doc(3)], &auto).is_ok());
    }

    #[test]
    fn rename_needs_a_region_a_real_page_and_text_in_the_pattern() {
        let tool = RenameByContentTool::new(ToolDeps::unavailable());
        let region = Region {
            x: 0.1,
            y: 0.1,
            width: 0.3,
            height: 0.1,
        };
        let ok = ToolOptions::RenameByContent(RenameByContentOptions {
            region: Some(region),
            ..RenameByContentOptions::default()
        });
        assert!(tool.validate(&[doc(2)], &ok).is_ok());
        let no_region = ToolOptions::defaults_for(ToolId::RenameByContent);
        assert!(tool.validate(&[doc(2)], &no_region).is_err());
        let far = ToolOptions::RenameByContent(RenameByContentOptions {
            region: Some(region),
            page: 5,
            ..RenameByContentOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc(2)], &far).unwrap_err().code,
            ErrorCode::RangeOutOfBounds
        );
    }

    #[test]
    fn text_from_a_page_becomes_one_line() {
        assert_eq!(
            one_line("  INV-1001 \r\n  Acme  Ltd\n"),
            "INV-1001 Acme Ltd"
        );
        assert_eq!(one_line(" \n "), "");
    }

    #[test]
    fn extracted_attachments_keep_a_safe_extension() {
        assert_eq!(safe_extension("report.PDF"), "pdf");
        assert_eq!(safe_extension("archive.tar.gz"), "gz");
        assert_eq!(safe_extension("noext"), "bin");
        assert_eq!(safe_extension("bad.e/x"), "bin");
        assert_eq!(safe_extension("dots."), "bin");
        assert_eq!(attachment_stem("report.pdf"), "report");
        assert_eq!(attachment_stem("noext"), "noext");
        assert_eq!(attachment_stem(".hidden"), ".hidden");
    }

    #[test]
    fn attachments_validate_per_action() {
        let tool = AttachmentsTool::new(ToolDeps::unavailable());
        assert!(
            tool.validate(&[doc(1)], &ToolOptions::defaults_for(ToolId::Attachments))
                .is_ok()
        );
        let add = ToolOptions::Attachments(AttachmentsOptions {
            action: AttachmentAction::Add,
            ..AttachmentsOptions::default()
        });
        assert!(tool.validate(&[doc(1)], &add).is_err());
        let delete = ToolOptions::Attachments(AttachmentsOptions {
            action: AttachmentAction::Delete,
            names: vec!["a.txt".into()],
            ..AttachmentsOptions::default()
        });
        assert!(tool.validate(&[doc(1)], &delete).is_ok());
    }

    #[test]
    fn page_labels_check_ranges_against_the_page_count() {
        let tool = PageLabelsTool::new(ToolDeps::unavailable());
        let range = |from_page| LabelRange {
            from_page,
            style: NumberStyle::RomanLower,
            ..LabelRange::default()
        };
        let ok = ToolOptions::PageLabels(PageLabelsOptions {
            ranges: vec![range(1)],
            remove_all: false,
        });
        assert!(tool.validate(&[doc(4)], &ok).is_ok());
        let bad = ToolOptions::PageLabels(PageLabelsOptions {
            ranges: vec![range(9)],
            remove_all: false,
        });
        assert!(tool.validate(&[doc(4)], &bad).is_err());
        assert!(
            tool.validate(&[doc(4)], &ToolOptions::defaults_for(ToolId::PageLabels))
                .is_err()
        );
    }
}
