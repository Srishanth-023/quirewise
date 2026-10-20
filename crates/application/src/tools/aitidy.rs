//! The AI part of PDF to Markdown (row 29): the language model tidies the prose of the text that
//! was read from the pages. Tables, pictures and code are never sent. A tidied piece is used only
//! when it has the same words as the original, give or take the few that joining and mending
//! change; otherwise the original piece stays.

use super::aikit::{DATA_RULE, Model, PROMPT_CHARS, same_words};
use crate::error::AppError;

/// A tidied piece may be this many percent of the original length, in bytes.
const GROWTH_PERCENT: usize = 130;

fn system_prompt() -> String {
    format!(
        "You tidy Markdown text that was read from the pages of a PDF. Join lines that the page width broke inside a sentence, \
mend words that were broken by a hyphen at the end of a line, and fix the heading levels so that # is the title and ## and ### \
follow the structure. Keep the Markdown marks of headings (#), lists (- or 1.) and emphasis. Do not change, add, remove, \
reorder or translate any word. {DATA_RULE} Answer with the tidied text only."
    )
}

/// True for a piece that must not be sent: tables, pictures, code and rules.
fn protected(block: &str) -> bool {
    let first = block.trim_start();
    first.starts_with('|')
        || first.starts_with("![")
        || first.starts_with("```")
        || first.starts_with("---")
        || first.starts_with('<')
}

/// Groups of consecutive pieces to send, as `(from, to)` indexes into the pieces, each group
/// within the budget. Protected pieces are in no group.
fn groups(blocks: &[&str]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut size = 0;
    for (i, block) in blocks.iter().enumerate() {
        let skip = protected(block) || block.trim().is_empty();
        let fits = size + block.len() + 2 <= PROMPT_CHARS;
        if skip || (start.is_some() && !fits) {
            if let Some(s) = start.take() {
                out.push((s, i));
            }
            size = 0;
        }
        if skip {
            continue;
        }
        if start.is_none() {
            start = Some(i);
        }
        size += block.len() + 2;
    }
    if let Some(s) = start {
        out.push((s, blocks.len()));
    }
    out
}

/// The tidied Markdown and the number of groups that were left as they were. `step` is called
/// with the number of the group done and the number of groups.
pub fn tidy(
    model: &Model<'_>,
    markdown: &str,
    step: &mut dyn FnMut(usize, usize),
) -> Result<(String, usize), AppError> {
    let blocks: Vec<&str> = markdown.split("\n\n").collect();
    let todo = groups(&blocks);
    let system = system_prompt();
    let mut result: Vec<Option<String>> = blocks.iter().map(|b| Some((*b).to_owned())).collect();
    let mut kept = 0;
    for (n, (from, to)) in todo.iter().enumerate() {
        let original = blocks[*from..*to].join("\n\n");
        let tokens = u32::try_from((original.len() / 2).clamp(300, 3_000)).unwrap_or(3_000);
        let tidied = match model.ask(&system, &original, tokens) {
            Ok(text) => Some(text),
            Err(e) if e.code == domain::ErrorCode::OutputVerifyFailed => None,
            Err(e) => return Err(e),
        };
        match tidied {
            Some(text)
                if same_words(&original, &text)
                    && text.len() * 100 <= original.len() * GROWTH_PERCENT =>
            {
                result[*from] = Some(text.trim().to_owned());
                for r in &mut result[*from + 1..*to] {
                    *r = None;
                }
            }
            _ => kept += 1,
        }
        step(n + 1, todo.len());
    }
    let joined: Vec<String> = result.into_iter().flatten().collect();
    Ok((joined.join("\n\n"), kept))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::CancellationToken;
    use crate::tools::aikit::testkit::Script;
    use domain::tools::ai::AiOptions;

    fn run(markdown: &str, answers: &[&str]) -> (String, usize, Script) {
        let llm = Script::new(answers);
        let cancel = CancellationToken::new();
        let ai = AiOptions::default();
        let model = Model {
            llm: &llm,
            ai: &ai,
            cancel: &cancel,
        };
        let (text, kept) = tidy(&model, markdown, &mut |_, _| {}).unwrap();
        (text, kept, llm)
    }

    #[test]
    fn prose_is_tidied_and_tables_and_pictures_are_not_sent() {
        let md = "# Report\n\nThe informa-\ntion was broken\nacross lines in this paragraph of text.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n![pic](p.png)\n\nSecond paragraph here with enough words to be a paragraph.";
        let (text, kept, llm) = run(
            md,
            &[
                "# Report\n\nThe information was broken across lines in this paragraph of text.",
                "Second paragraph here with enough words to be a paragraph.",
            ],
        );
        assert_eq!(kept, 0);
        assert_eq!(
            text,
            "# Report\n\nThe information was broken across lines in this paragraph of text.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n![pic](p.png)\n\nSecond paragraph here with enough words to be a paragraph."
        );
        let asked = llm.asked.lock().unwrap();
        assert_eq!(asked.len(), 2);
        assert!(
            asked
                .iter()
                .all(|r| !r.user.contains('|') && !r.user.contains("!["))
        );
        assert!(asked[0].system.contains("Do not change, add, remove"));
    }

    #[test]
    fn an_answer_with_other_words_leaves_the_original() {
        let md =
            "The quarterly figures show that sales rose in every region of the country this year.";
        let (text, kept, _) = run(
            md,
            &["Sales are great! Buy our new product today and save a lot of money, call now."],
        );
        assert_eq!(text, md);
        assert_eq!(kept, 1);
    }

    #[test]
    fn an_empty_answer_leaves_the_original_too() {
        let md = "Some text that is long enough to be sent to the model for tidying up.";
        let (text, kept, _) = run(md, &[" "]);
        assert_eq!(text, md);
        assert_eq!(kept, 1);
    }

    #[test]
    fn long_text_goes_in_several_requests_that_fit() {
        let para = format!("{}.", "word ".repeat(300));
        let md = (0..6)
            .map(|_| para.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        // Three paragraphs fit one request: two requests, each answered with its three.
        let group = vec![para.clone(); 3].join(
            "

",
        );
        let answers: Vec<String> = vec![group.clone(), group];
        let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
        let (text, kept, llm) = run(&md, &refs);
        let asked = llm.asked.lock().unwrap();
        assert_eq!(asked.len(), 2);
        assert!(asked.iter().all(|r| r.user.len() <= PROMPT_CHARS));
        assert_eq!(kept, 0);
        assert_eq!(text.matches("word").count(), 6 * 300);
    }

    #[test]
    fn nothing_to_send_means_no_request() {
        let (text, kept, llm) = run("| a |\n|---|\n| 1 |", &[]);
        assert_eq!(text, "| a |\n|---|\n| 1 |");
        assert_eq!(kept, 0);
        assert!(llm.asked.lock().unwrap().is_empty());
    }
}
