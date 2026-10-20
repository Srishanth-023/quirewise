//! Smart Split (Appendix A row 12): the language model reads the first words of every page and
//! says where new parts start, following an instruction in the person's own words. The parts are
//! shown first; what was shown is what is made.

use domain::tools::ToolGroup;
use domain::tools::ai::{MAX_PARTS, MAX_TITLE_CHARS, SmartSplitOptions, SplitPart};
use domain::{ErrorCode, PageIndex, ToolId, ToolOptions};
use serde_json::Value;
use std::path::Path;

use super::aikit::{DATA_RULE, Model, snippet};
use super::common::{Progress, ToolDeps, first_input, page_count, publish_pdf, wrong_options};
use crate::error::AppError;
use crate::ports::TextExtractor;
use crate::runtime::Password;
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Pages whose first words go into one request.
const WINDOW: u32 = 40;
/// Characters of each page that the model sees.
const SNIPPET_CHARS: usize = 120;
/// Tokens the answer may have.
const ANSWER_TOKENS: u32 = 1_500;

fn system_prompt(later: Option<&str>) -> String {
    let mut text = format!(
        "You split a document into parts. You are given the first words of each page, as lines like [12] words. \
Decide on which pages a new part starts, following the instruction. {DATA_RULE} \
Answer with one JSON object: {{\"parts\":[{{\"start\":<page number>,\"title\":\"<what the part is about>\"}}]}}. \
Use only page numbers from the list, in ascending order. A title has at most 8 words, in the language of the document, \
with no quotation marks."
    );
    match later {
        None => text.push_str(" Page numbers start at the first line; the first part starts there."),
        Some(open) => text.push_str(&format!(
            " The pages before these belong to a part called \"{open}\". List only the pages where a NEW part starts; the list may be empty."
        )),
    }
    text
}

fn clean_title(title: &str) -> String {
    let line: String = title
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = line.trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace());
    trimmed.chars().take(MAX_TITLE_CHARS.min(100)).collect()
}

/// The starts the model named inside `first..=last` (pages from 1), in order, once each.
fn read_starts(value: &Value, first: u32, last: u32) -> Vec<(u32, String)> {
    let mut starts: Vec<(u32, String)> = Vec::new();
    let Some(list) = value["parts"].as_array() else {
        return starts;
    };
    for part in list {
        let start = part["start"]
            .as_u64()
            .or_else(|| part["start"].as_str().and_then(|s| s.trim().parse().ok()));
        let Some(start) = start.and_then(|s| u32::try_from(s).ok()) else {
            continue;
        };
        if start < first || start > last || starts.iter().any(|(s, _)| *s == start) {
            continue;
        }
        let title = part["title"].as_str().map(clean_title).unwrap_or_default();
        starts.push((start, title));
    }
    starts.sort_by_key(|(s, _)| *s);
    starts
}

/// The parts the model proposes for `count` pages. `step` is called once for each page read.
pub fn propose(
    model: &Model<'_>,
    text: &dyn TextExtractor,
    path: &Path,
    password: Option<&Password>,
    count: u32,
    instruction: &str,
    step: &mut dyn FnMut(),
) -> Result<Vec<SplitPart>, AppError> {
    let mut starts: Vec<(u32, String)> = Vec::new();
    let mut seen_text = false;
    let mut first = 1;
    while first <= count {
        let last = (first + WINDOW - 1).min(count);
        let mut lines = String::new();
        for page in first..=last {
            model.cancel.check()?;
            let words = text.page_text(path, password, PageIndex::new(page - 1))?;
            step();
            let snip = snippet(&words, SNIPPET_CHARS);
            seen_text |= !snip.is_empty();
            lines.push_str(&format!(
                "[{page}] {}\n",
                if snip.is_empty() { "(no text)" } else { &snip }
            ));
        }
        let open = (first > 1).then(|| {
            starts
                .last()
                .map_or_else(|| "the first part".to_owned(), |(_, t)| t.clone())
        });
        let system = system_prompt(open.as_deref());
        let user = format!("Instruction: {}\n\nPages:\n{lines}", instruction.trim());
        let answer = model.ask_json(&system, &user, ANSWER_TOKENS)?;
        starts.extend(read_starts(&answer, first, last));
        first = last + 1;
    }
    if !seen_text {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            "These pages have no text to read. Run OCR on the file first, then split it.",
        ));
    }
    if starts.first().is_none_or(|(s, _)| *s != 1) {
        starts.insert(0, (1, String::new()));
    }
    if starts.len() > MAX_PARTS {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            format!("The instruction would make more than {MAX_PARTS} parts. Make it less fine."),
        ));
    }
    let mut parts = Vec::with_capacity(starts.len());
    for (i, (start, title)) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map_or(count, |(next, _)| next - 1);
        parts.push(SplitPart {
            start: *start,
            end,
            title: if title.is_empty() {
                format!("Part {}", i + 1)
            } else {
                title.clone()
            },
        });
    }
    if parts.len() == 1 && count > 1 {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            "The model found no place to split. Describe the parts more closely, for example: a new file where a new invoice starts.",
        ));
    }
    Ok(parts)
}

/// The parts the model proposes for a document, for the preview that shows them before anything
/// is made.
pub fn preview(
    llm: &dyn crate::ports::LlmEngine,
    text: &dyn TextExtractor,
    path: &Path,
    password: Option<&Password>,
    count: u32,
    options: &SmartSplitOptions,
    cancel: &crate::runtime::CancellationToken,
) -> Result<Vec<SplitPart>, AppError> {
    options.ai.validate()?;
    options.check_instruction()?;
    let model = Model {
        llm,
        ai: &options.ai,
        cancel,
    };
    model.ready()?;
    propose(
        &model,
        text,
        path,
        password,
        count,
        &options.instruction,
        &mut || (),
    )
}

/// Splits a document where a language model says new parts start.
pub struct SmartSplitTool {
    deps: ToolDeps,
}

impl SmartSplitTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn options(opts: &ToolOptions) -> Result<&SmartSplitOptions, AppError> {
    match opts {
        ToolOptions::SmartSplit(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for SmartSplitTool {
    fn id(&self) -> ToolId {
        ToolId::SmartSplit
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Intelligence
    }
    fn label(&self) -> &'static str {
        "part"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(SmartSplitOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let count = page_count(input);
        let o = options(opts)?;
        o.validate(count)?;
        let reading = if o.parts.is_empty() {
            u64::from(count)
        } else {
            0
        };
        Ok(Plan {
            tool: ToolId::SmartSplit,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: reading + u64::from(count),
        })
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let o = options(&plan.options)?;
        let count = page_count(input);
        let mut progress = Progress::start(ctx, plan.total_units);
        let parts = if o.parts.is_empty() {
            let model = Model {
                llm: self.deps.llm.as_ref(),
                ai: &o.ai,
                cancel: ctx.cancel,
            };
            model.ready()?;
            let parts = propose(
                &model,
                self.deps.text.as_ref(),
                &input.path,
                input.password.as_ref(),
                count,
                &o.instruction,
                &mut || progress.advance(1),
            )?;
            domain::tools::ai::check_parts(&parts, count)?;
            parts
        } else {
            o.parts.clone()
        };
        let mut outputs = Vec::with_capacity(parts.len());
        for (i, part) in parts.iter().enumerate() {
            ctx.cancel.check()?;
            let pages: Vec<PageIndex> = (part.start - 1..part.end).map(PageIndex::new).collect();
            let label = if part.title.trim().is_empty() {
                format!("part {}", i + 1)
            } else {
                part.title.clone()
            };
            outputs.push(publish_pdf(
                ctx,
                &self.deps,
                input,
                &label,
                part.end - part.start + 1,
                &mut |tmp| {
                    self.deps
                        .writer
                        .copy_pages(&input.path, input.password.as_ref(), &pages, tmp)
                },
            )?);
            progress.advance(u64::from(part.end - part.start + 1));
        }
        Ok(outputs)
    }
}

#[cfg(test)]
mod tests;
