//! Translate PDF (Appendix A row 57): the text of a document is translated by the language model
//! on this computer. With the layout kept, each block of text is removed and the translation is
//! written in its place at a size that fits; without it, the translation is a new document of
//! flowing text.

use domain::layout::{Block, Detection, PageContent, analyze, plain};
use domain::stamp::Rgb;
use domain::tools::ToolGroup;
use domain::tools::ai::{Language, TranslatePdfOptions};
use domain::tools::common::Region;
use domain::{Duplicates, ErrorCode, PageIndex, ToolId, ToolOptions};
use std::collections::HashMap;

use super::aikit::{DATA_RULE, Model, PROMPT_CHARS};
use super::common::{
    Progress, Scratch, ToolDeps, first_input, kept_protection, page_count, publish_pdf_checked,
    wrong_options,
};
use crate::error::AppError;
use crate::ports::{DocBlock, PackageOptions, PdfEdit, RedactPage, RedactPlan, TextReplacement};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Characters of text translated in one request: the answer is as long as the question.
const BATCH_CHARS: usize = PROMPT_CHARS / 2;
/// A translation longer than this many times the original (plus a margin) is a runaway.
const MAX_GROWTH: usize = 5;
/// The share of texts that may fail before the whole run fails.
const MAX_FAILED_SHARE: f64 = 0.5;
/// Space added around a block when it is cleared, as a fraction of the page.
const PAD: f32 = 0.002;
/// A4 in points, for a document whose page size is not known.
const A4: (f32, f32) = (595.28, 841.89);

/// A block of text and its translation, when there is one.
type Translated = Vec<(Piece, Option<String>)>;

/// The translated blocks of one page.
type PageTranslation = (PageIndex, Translated);

/// What kind of text a piece is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A heading.
    Heading,
    /// A paragraph.
    Paragraph,
    /// A list item.
    Bullet,
    /// A cell of a table.
    Cell,
}

/// A block of text of a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// What it is.
    pub kind: Kind,
    /// The text.
    pub text: String,
    /// Where it is, as fractions of the displayed page.
    pub region: Region,
    /// The size of its text, in points.
    pub size_pt: f32,
    /// Bold throughout.
    pub bold: bool,
}

fn fraction(rect: domain::layout::Rect, w: f32, h: f32) -> Region {
    let x0 = (rect.x0 / w - PAD).clamp(0.0, 1.0);
    let y0 = (rect.y0 / h - PAD).clamp(0.0, 1.0);
    let x1 = (rect.x1 / w + PAD).clamp(0.0, 1.0);
    let y1 = (rect.y1 / h + PAD).clamp(0.0, 1.0);
    Region {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0.001),
        height: (y1 - y0).max(0.001),
    }
}

/// The blocks of text of a page, in reading order, with their places.
pub fn pieces_of(content: PageContent) -> Vec<Piece> {
    let (w, h) = (content.width.max(1.0), content.height.max(1.0));
    let layouts = analyze(vec![content], Detection::Lattice);
    let mut out = Vec::new();
    for block in layouts.iter().flat_map(|l| &l.blocks) {
        match block {
            Block::Heading { spans, rect, .. }
            | Block::Paragraph { spans, rect, .. }
            | Block::ListItem { spans, rect, .. } => {
                let text = plain(spans);
                if text.trim().is_empty() || spans.is_empty() {
                    continue;
                }
                let kind = match block {
                    Block::Heading { .. } => Kind::Heading,
                    Block::ListItem { .. } => Kind::Bullet,
                    _ => Kind::Paragraph,
                };
                out.push(Piece {
                    kind,
                    text: text.split_whitespace().collect::<Vec<_>>().join(" "),
                    region: fraction(*rect, w, h),
                    size_pt: spans.iter().map(|s| s.size).sum::<f32>() / spans.len() as f32,
                    bold: spans.iter().all(|s| s.bold),
                });
            }
            Block::Table(table) => {
                for (r, row) in table.rows.iter().enumerate() {
                    let top = table.row_edges.get(r).copied().unwrap_or(table.rect.y0);
                    let bottom = table.row_edges.get(r + 1).copied().unwrap_or(table.rect.y1);
                    for (c, cell) in row.iter().enumerate() {
                        if cell.text.trim().is_empty() {
                            continue;
                        }
                        let left = table.column_edges.get(c).copied().unwrap_or(table.rect.x0);
                        let right = table
                            .column_edges
                            .get(c + 1)
                            .copied()
                            .unwrap_or(table.rect.x1);
                        out.push(Piece {
                            kind: Kind::Cell,
                            text: cell.text.split_whitespace().collect::<Vec<_>>().join(" "),
                            region: fraction(
                                domain::layout::Rect::new(left, top, right, bottom),
                                w,
                                h,
                            ),
                            size_pt: ((bottom - top) * 0.6).clamp(6.0, 10.0),
                            bold: cell.bold,
                        });
                    }
                }
            }
            Block::Picture(_) => {}
        }
    }
    out
}

/// True when a text has something to translate: a letter.
fn translatable(text: &str) -> bool {
    text.chars().any(char::is_alphabetic)
}

fn translate_prompt(language: &Language) -> String {
    format!(
        "You translate texts into {}. {DATA_RULE} You get one JSON object {{\"texts\":[{{\"id\":1,\"text\":\"...\"}}]}}. \
Translate every text. Keep numbers, dates, e-mail addresses, web addresses, product codes and the names of people and companies as they are. \
Do not shorten, explain or add anything. Answer with one JSON object {{\"texts\":[{{\"id\":1,\"text\":\"<translation>\"}}]}} with the same ids.",
        language.name
    )
}

fn single_prompt(language: &Language) -> String {
    format!(
        "You translate a text into {}. {DATA_RULE} Keep numbers, dates, e-mail addresses, web addresses, product codes and the names of people and companies as they are. \
Do not shorten, explain or add anything. Answer with the translation only.",
        language.name
    )
}

/// True for the failures that no retry mends.
fn fatal(e: &AppError) -> bool {
    e.code != ErrorCode::OutputVerifyFailed
}

fn plausible(source: &str, translation: &str) -> bool {
    let t = translation.trim();
    !t.is_empty() && t.len() <= source.len() * MAX_GROWTH + 80
}

/// Translates texts. A text that comes back missing or unusable is asked for again alone; one
/// that fails then too is `None`. Equal texts are asked once (`seen` carries them across pages).
pub fn translate_texts(
    model: &Model<'_>,
    language: &Language,
    texts: &[String],
    seen: &mut HashMap<String, Option<String>>,
) -> Result<Vec<Option<String>>, AppError> {
    let mut todo: Vec<&String> = Vec::new();
    for t in texts {
        if translatable(t) && !seen.contains_key(t) && !todo.contains(&t) {
            todo.push(t);
        }
    }
    let system = translate_prompt(language);
    let mut start = 0;
    while start < todo.len() {
        let mut end = start;
        let mut chars = 0;
        while end < todo.len() && (end == start || chars + todo[end].len() <= BATCH_CHARS) {
            chars += todo[end].len();
            end += 1;
        }
        let batch = &todo[start..end];
        let request = serde_json::json!({
            "texts": batch.iter().enumerate()
                .map(|(i, t)| serde_json::json!({ "id": i + 1, "text": t }))
                .collect::<Vec<_>>()
        });
        let tokens = u32::try_from(chars.clamp(300, 3_000)).unwrap_or(3_000);
        let answered = match model.ask_json(&system, &request.to_string(), tokens) {
            Ok(v) => Some(v),
            Err(e) if fatal(&e) => return Err(e),
            Err(_) => None,
        };
        let mut by_id: HashMap<u64, String> = HashMap::new();
        if let Some(list) = answered.as_ref().and_then(|v| v["texts"].as_array()) {
            for item in list {
                let id = item["id"]
                    .as_u64()
                    .or_else(|| item["id"].as_str().and_then(|s| s.trim().parse().ok()));
                if let (Some(id), Some(text)) = (id, item["text"].as_str()) {
                    by_id.insert(id, text.trim().to_owned());
                }
            }
        }
        for (i, source) in batch.iter().enumerate() {
            let got = by_id
                .get(&(i as u64 + 1))
                .filter(|t| plausible(source, t))
                .cloned();
            let result = match got {
                Some(t) => Some(t),
                None => match model.ask(&single_prompt(language), source, tokens) {
                    Ok(t) if plausible(source, &t) => Some(t),
                    Ok(_) => None,
                    Err(e) if fatal(&e) => return Err(e),
                    Err(_) => None,
                },
            };
            seen.insert((*source).clone(), result);
        }
        start = end;
    }
    Ok(texts
        .iter()
        .map(|t| seen.get(t).cloned().flatten())
        .collect())
}

/// Translates a document.
pub struct TranslatePdfTool {
    deps: ToolDeps,
}

impl TranslatePdfTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn pages_of(options: &TranslatePdfOptions, count: u32) -> Result<Vec<PageIndex>, AppError> {
    let mut pages: Vec<PageIndex> = options
        .pages
        .resolve(count, Duplicates::Reject)?
        .pages()
        .to_vec();
    pages.sort_by_key(|p| p.get());
    Ok(pages)
}

impl Tool for TranslatePdfTool {
    fn id(&self) -> ToolId {
        ToolId::TranslatePdf
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Intelligence
    }
    fn label(&self) -> &'static str {
        "translated"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(TranslatePdfOptions)
    }
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::TranslatePdf(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        let pages = pages_of(options, page_count(first_input(inputs)?))?;
        Ok(Plan {
            tool: ToolId::TranslatePdf,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: pages.len() as u64 + 1,
        })
    }
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::TranslatePdf(options) = &plan.options else {
            return Err(wrong_options());
        };
        let language = options.validate()?;
        let input = first_input(&plan.inputs)?;
        let count = page_count(input);
        let pages = pages_of(options, count)?;
        let model = Model {
            llm: self.deps.llm.as_ref(),
            ai: &options.ai,
            cancel: ctx.cancel,
        };
        model.ready()?;
        let mut progress = Progress::start(ctx, plan.total_units);

        // Read and translate page by page.
        let mut done: Vec<PageTranslation> = Vec::new();
        let mut seen: HashMap<String, Option<String>> = HashMap::new();
        let (mut wanted, mut failed) = (0usize, 0usize);
        for page in &pages {
            ctx.cancel.check()?;
            let content =
                self.deps
                    .layout
                    .page_content(&input.path, input.password.as_ref(), *page)?;
            let pieces = pieces_of(content);
            let texts: Vec<String> = pieces.iter().map(|p| p.text.clone()).collect();
            let translated = translate_texts(&model, language, &texts, &mut seen)?;
            let mut paired = Vec::with_capacity(pieces.len());
            for (piece, result) in pieces.into_iter().zip(translated) {
                if translatable(&piece.text) {
                    wanted += 1;
                    failed += usize::from(result.is_none());
                }
                paired.push((piece, result));
            }
            if !paired.is_empty() {
                done.push((*page, paired));
            }
            progress.advance(1);
        }
        if wanted == 0 {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "These pages have no text to translate. Run OCR on the file first.",
            ));
        }
        if failed as f64 > wanted as f64 * MAX_FAILED_SHARE {
            return Err(AppError::new(
                ErrorCode::OutputVerifyFailed,
                "The language model could not translate most of the text. Try a larger model.",
            ));
        }
        if failed > 0 {
            ctx.logger
                .log(crate::ports::LogLevel::Warn, "translate.some-text-kept");
        }

        let out = if options.preserve_layout {
            self.in_place(ctx, input, language, count, &done)?
        } else {
            self.flowing(ctx, input, language, &done)?
        };
        progress.advance(1);
        Ok(vec![out])
    }
}

impl TranslatePdfTool {
    fn label_for(&self, language: &Language) -> String {
        format!("translated {}", language.code)
    }

    /// The pages as they were, with each translated block cleared and rewritten.
    fn in_place(
        &self,
        ctx: &JobContext<'_>,
        input: &InputDoc,
        language: &Language,
        count: u32,
        done: &[PageTranslation],
    ) -> Result<OutputArtifact, AppError> {
        let white =
            Rgb::parse("#FFFFFF").map_err(|_| AppError::internal("reading the color white"))?;
        let mut items: Vec<TextReplacement> = Vec::new();
        let mut cleared: Vec<RedactPage> = Vec::new();
        for (page, pieces) in done {
            let mut regions = Vec::new();
            for (piece, translation) in pieces {
                let Some(text) = translation else { continue };
                regions.push(piece.region);
                items.push(TextReplacement {
                    page: *page,
                    region: piece.region,
                    text: text.clone(),
                    size_pt: piece.size_pt,
                    bold: piece.bold || piece.kind == Kind::Heading,
                });
            }
            if !regions.is_empty() {
                cleared.push(RedactPage {
                    page: *page,
                    regions,
                });
            }
        }
        let protection = kept_protection(&self.deps, input)?;
        let password = protection.as_ref().map(|p| &p.password);
        publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            &self.label_for(language),
            Some(count),
            password,
            &mut |tmp| {
                let unlocked = Scratch::new(ctx, "pdf");
                self.deps
                    .security
                    .unlock(&input.path, input.password.as_ref(), unlocked.path())?;
                ctx.cancel.check()?;
                // The old text is removed from the page, not just covered. Pages that cannot be
                // cleaned keep it under the patch that the new text is drawn on.
                let removed = Scratch::new(ctx, "pdf");
                self.deps.editor.redact(
                    unlocked.path(),
                    &RedactPlan {
                        pages: cleared.clone(),
                        fill: white,
                    },
                    removed.path(),
                )?;
                ctx.cancel.check()?;
                let written = Scratch::new(ctx, "pdf");
                self.deps.editor.apply(
                    removed.path(),
                    &PdfEdit::ReplaceText(items.clone()),
                    written.path(),
                )?;
                ctx.cancel.check()?;
                self.deps.packager.package(
                    written.path(),
                    None,
                    &PackageOptions {
                        keep_protection: protection.clone(),
                        ..PackageOptions::new(false, true)
                    },
                    tmp,
                )
            },
        )
    }

    /// A new document of flowing text.
    fn flowing(
        &self,
        ctx: &JobContext<'_>,
        input: &InputDoc,
        language: &Language,
        done: &[PageTranslation],
    ) -> Result<OutputArtifact, AppError> {
        let mut blocks: Vec<DocBlock> = Vec::new();
        for (_, pieces) in done {
            for (piece, translation) in pieces {
                let text = translation.clone().unwrap_or_else(|| piece.text.clone());
                blocks.push(match piece.kind {
                    Kind::Heading => DocBlock::Heading(text),
                    Kind::Paragraph => DocBlock::Paragraph(text),
                    Kind::Bullet | Kind::Cell => DocBlock::Bullet(text),
                });
            }
        }
        let size = self
            .deps
            .reader
            .page_sizes(&input.path, input.password.as_ref())?
            .first()
            .map_or(A4, |s| (s.width_pt.max(200.0), s.height_pt.max(200.0)));
        publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            &self.label_for(language),
            None,
            None,
            &mut |tmp| {
                self.deps
                    .textdoc
                    .write(&blocks, &input.stem(), size.0, size.1, tmp)
            },
        )
    }
}

#[cfg(test)]
mod tests;
