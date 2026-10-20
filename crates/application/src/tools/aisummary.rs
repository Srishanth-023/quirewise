//! AI Summarizer (Appendix A row 56): a summary of a document, written by the language model on
//! this computer in the chosen language, as a PDF or a Markdown file.
//!
//! A long document is summarized in pieces that fit a request, and the summaries of the pieces
//! are summarized again until one request holds them.

use domain::tools::ToolGroup;
use domain::tools::ai::{AiSummarizerOptions, Language, SummaryFormat, SummaryLength};
use domain::{Duplicates, ErrorCode, PageIndex, ToolId, ToolOptions};

use super::aikit::{Chunk, DATA_RULE, Model, PROMPT_CHARS, chunk_pages, markdown_blocks};
use super::common::{
    Progress, ToolDeps, first_input, page_count, publish_pdf_checked, wrong_options,
};
use super::export::write_bytes;
use crate::error::AppError;
use crate::ports::DocBlock;
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Tokens a summary of a piece may have.
const PIECE_TOKENS: u32 = 450;
/// How many times summaries are summarized again before the rest is cut.
const MAX_ROUNDS: usize = 4;
/// A4 in points, for a document whose page size is not known.
const A4: (f32, f32) = (595.28, 841.89);

fn piece_prompt(language: &Language) -> String {
    format!(
        "You summarize one part of a long document for someone who will read the summaries instead of the document. {DATA_RULE} \
Write in {}. Keep names, numbers, dates and decisions. Answer with the summary only: 3 to 6 sentences, no preface.",
        language.name
    )
}

fn final_prompt(language: &Language, length: SummaryLength) -> (String, u32) {
    let (form, tokens) = match length {
        SummaryLength::Short => ("one paragraph of 3 to 4 sentences", 350),
        SummaryLength::Medium => ("about three short paragraphs, 150 to 250 words in all", 800),
        SummaryLength::Long => (
            "structured Markdown: a heading (## ) for each main topic with bullet points (- ) under it, 400 to 600 words in all",
            1_600,
        ),
    };
    (
        format!(
            "You write a summary of a document. {DATA_RULE} Write in {}. Write {form}. Use only what the text says; do not add facts. \
Plain Markdown only (no tables, no pictures). No preface and no closing remark.",
            language.name
        ),
        tokens,
    )
}

/// The summary of the chunks, in Markdown. `step` is called with the number of pages each
/// request has covered, to move a progress bar.
pub fn summarize(
    model: &Model<'_>,
    chunks: &[Chunk],
    language: &Language,
    length: SummaryLength,
    step: &mut dyn FnMut(u32),
) -> Result<String, AppError> {
    let (final_system, final_tokens) = final_prompt(language, length);
    if let [only] = chunks {
        let text = model.ask(
            &final_system,
            &format!("Document text:\n\n{}", only.text),
            final_tokens,
        )?;
        step(only.last - only.first + 1);
        return Ok(text);
    }
    let piece_system = piece_prompt(language);
    let mut summaries: Vec<String> = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        summaries.push(model.ask(
            &piece_system,
            &format!("Document text:\n\n{}", chunk.text),
            PIECE_TOKENS,
        )?);
        step(chunk.last - chunk.first + 1);
    }
    for _ in 0..MAX_ROUNDS {
        let joined = summaries.join("\n\n");
        if joined.len() <= PROMPT_CHARS || summaries.len() < 2 {
            break;
        }
        // Summarize neighbors together until the whole fits.
        let numbered: Vec<(u32, String)> = summaries
            .iter()
            .enumerate()
            .map(|(i, s)| (u32::try_from(i + 1).unwrap_or(u32::MAX), s.clone()))
            .collect();
        let groups = chunk_pages(&numbered, PROMPT_CHARS);
        if groups.len() >= summaries.len() {
            break;
        }
        summaries = groups
            .iter()
            .map(|g| {
                model.ask(
                    &piece_system,
                    &format!("Summaries of consecutive parts:\n\n{}", g.text),
                    PIECE_TOKENS,
                )
            })
            .collect::<Result<_, _>>()?;
    }
    let mut joined = summaries.join("\n\n");
    if joined.len() > PROMPT_CHARS {
        let mut cut = PROMPT_CHARS;
        while !joined.is_char_boundary(cut) {
            cut -= 1;
        }
        joined.truncate(cut);
    }
    model.ask(
        &final_system,
        &format!("Summaries of the parts of the document, in order:\n\n{joined}"),
        final_tokens,
    )
}

/// Writes a summary of a document.
pub struct AiSummarizerTool {
    deps: ToolDeps,
}

impl AiSummarizerTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn pages_of(options: &AiSummarizerOptions, count: u32) -> Result<Vec<PageIndex>, AppError> {
    let mut pages: Vec<PageIndex> = options
        .pages
        .resolve(count, Duplicates::Reject)?
        .pages()
        .to_vec();
    pages.sort_by_key(|p| p.get());
    Ok(pages)
}

impl Tool for AiSummarizerTool {
    fn id(&self) -> ToolId {
        ToolId::AiSummarizer
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Intelligence
    }
    fn label(&self) -> &'static str {
        "summary"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(AiSummarizerOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::AiSummarizer(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let pages = pages_of(options, page_count(first_input(inputs)?))?;
        Ok(Plan {
            tool: ToolId::AiSummarizer,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: 2 * pages.len() as u64 + 2,
        })
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::AiSummarizer(options) = &plan.options else {
            return Err(wrong_options());
        };
        let language = options.validate()?;
        let input = first_input(&plan.inputs)?;
        let pages = pages_of(options, page_count(input))?;
        let model = Model {
            llm: self.deps.llm.as_ref(),
            ai: &options.ai,
            cancel: ctx.cancel,
        };
        model.ready()?;
        let mut progress = Progress::start(ctx, plan.total_units);

        let mut texts: Vec<(u32, String)> = Vec::with_capacity(pages.len());
        for page in &pages {
            ctx.cancel.check()?;
            let text = self
                .deps
                .text
                .page_text(&input.path, input.password.as_ref(), *page)?;
            texts.push((page.get() + 1, text));
            progress.advance(1);
        }
        let chunks = chunk_pages(&texts, PROMPT_CHARS);
        if chunks.is_empty() {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "These pages have no text to summarize. Run OCR on the file first.",
            ));
        }
        let mut covered = 0u64;
        let summary = summarize(&model, &chunks, language, options.length, &mut |n| {
            covered += u64::from(n);
            progress.advance(u64::from(n));
        })?;
        // Pages without text were never asked about; the bar still ends where it should.
        progress.advance((pages.len() as u64).saturating_sub(covered));

        let out = match options.format {
            SummaryFormat::Markdown => {
                let text = format!("# {}\n\n{summary}\n", input.stem());
                let dest = ctx.output_path(input, self.label(), "md", |_| {})?;
                write_bytes(ctx, &dest, text.as_bytes())?
            }
            SummaryFormat::Pdf => {
                let size = self
                    .deps
                    .reader
                    .page_sizes(&input.path, input.password.as_ref())?
                    .first()
                    .map_or(A4, |s| {
                        (
                            s.width_pt.min(s.height_pt).max(200.0),
                            s.width_pt.max(s.height_pt).max(200.0),
                        )
                    });
                let mut blocks = vec![DocBlock::Title(input.stem())];
                blocks.extend(markdown_blocks(&summary));
                publish_pdf_checked(
                    ctx,
                    &self.deps,
                    input,
                    self.label(),
                    None,
                    None,
                    &mut |tmp| {
                        self.deps
                            .textdoc
                            .write(&blocks, &input.stem(), size.0, size.1, tmp)
                    },
                )?
            }
        };
        progress.advance(2);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests;
