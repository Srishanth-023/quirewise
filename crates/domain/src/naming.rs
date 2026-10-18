//! Output naming: token patterns (Appendix C.2), file name sanitizing and collision handling.

use core::fmt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Default pattern: source name followed by the tool label, for example `report (compressed)`.
pub const DEFAULT_PATTERN: &str = "{filename} ({tool})";

const MAX_STEM_CHARS: usize = 120;
const MAX_COLLISION_ATTEMPTS: u32 = 10_000;
const FALLBACK_STEM: &str = "output";
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A substitution token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    /// Source name without extension.
    Filename,
    /// Tool label.
    Tool,
    /// `YYYY-MM-DD`.
    Date,
    /// `HHmmss`.
    Time,
    /// Sequence number, zero padded to the given width (0 means no padding).
    Sequence(u8),
    /// Current page (one-based).
    Page,
    /// Total pages.
    Pages,
    /// Bookmark title.
    Bookmark,
    /// Text read from a region.
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Token(Token),
}

/// A validated naming pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
#[schemars(with = "String")]
pub struct OutputNamingPattern {
    source: String,
    segments: Vec<Segment>,
}

/// Local date and time, supplied by the caller so the domain stays free of clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTimeParts {
    /// Full year.
    pub year: u16,
    /// Month, 1 to 12.
    pub month: u8,
    /// Day of month, 1 to 31.
    pub day: u8,
    /// Hour, 0 to 23.
    pub hour: u8,
    /// Minute, 0 to 59.
    pub minute: u8,
    /// Second, 0 to 59.
    pub second: u8,
}

impl DateTimeParts {
    fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn time(&self) -> String {
        format!("{:02}{:02}{:02}", self.hour, self.minute, self.second)
    }
}

/// Values available when rendering a pattern. Missing values make a token an error.
#[derive(Debug, Clone, Copy)]
pub struct NameContext<'a> {
    /// Source file name without extension.
    pub filename: &'a str,
    /// Tool label.
    pub tool: &'a str,
    /// Timestamp for `{date}` and `{time}`.
    pub now: DateTimeParts,
    /// Sequence number for `{n}`.
    pub sequence: Option<u32>,
    /// Current page for `{page}`.
    pub page: Option<u32>,
    /// Page count for `{pages}`.
    pub pages: Option<u32>,
    /// Bookmark title for `{bookmark}`.
    pub bookmark: Option<&'a str>,
    /// Region text for `{text}`.
    pub text: Option<&'a str>,
}

impl OutputNamingPattern {
    /// Parses a pattern. Unknown tokens and unbalanced braces are rejected.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let invalid = |reason: String| DomainError::option("naming pattern", reason);
        let mut segments: Vec<Segment> = Vec::new();
        let mut literal = String::new();
        let mut rest = input;
        let flush = |literal: &mut String, segments: &mut Vec<Segment>| {
            if !literal.is_empty() {
                segments.push(Segment::Literal(core::mem::take(literal)));
            }
        };
        while let Some(c) = rest.chars().next() {
            if c == '{' {
                let end = rest
                    .find('}')
                    .ok_or_else(|| invalid("a { has no closing }".to_owned()))?;
                let name = rest.get(1..end).unwrap_or_default();
                flush(&mut literal, &mut segments);
                segments.push(Segment::Token(parse_brace_token(name).map_err(invalid)?));
                rest = rest.get(end + 1..).unwrap_or_default();
            } else if c == '[' {
                if let Some((alias, len)) = match_alias(rest) {
                    flush(&mut literal, &mut segments);
                    segments.extend(alias);
                    rest = rest.get(len..).unwrap_or_default();
                } else {
                    literal.push(c);
                    rest = rest.get(c.len_utf8()..).unwrap_or_default();
                }
            } else if c == '}' {
                return Err(invalid("a } has no opening {".to_owned()));
            } else {
                literal.push(c);
                rest = rest.get(c.len_utf8()..).unwrap_or_default();
            }
        }
        flush(&mut literal, &mut segments);
        if segments.is_empty() {
            return Err(invalid("the pattern is empty".to_owned()));
        }
        Ok(Self {
            source: input.to_owned(),
            segments,
        })
    }

    /// The text the pattern was parsed from.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Renders and sanitizes the file name stem (no extension).
    pub fn render(&self, ctx: &NameContext<'_>) -> Result<String, DomainError> {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(s) => out.push_str(s),
                Segment::Token(t) => out.push_str(&token_value(*t, ctx)?),
            }
        }
        Ok(sanitize_stem(&out))
    }
}

impl Default for OutputNamingPattern {
    fn default() -> Self {
        // The default pattern is a compile-time constant that always parses.
        Self {
            source: DEFAULT_PATTERN.to_owned(),
            segments: vec![
                Segment::Token(Token::Filename),
                Segment::Literal(" (".to_owned()),
                Segment::Token(Token::Tool),
                Segment::Literal(")".to_owned()),
            ],
        }
    }
}

impl fmt::Display for OutputNamingPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl TryFrom<String> for OutputNamingPattern {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<OutputNamingPattern> for String {
    fn from(value: OutputNamingPattern) -> Self {
        value.source
    }
}

fn parse_brace_token(name: &str) -> Result<Token, String> {
    match name {
        "filename" => Ok(Token::Filename),
        "tool" => Ok(Token::Tool),
        "date" => Ok(Token::Date),
        "time" => Ok(Token::Time),
        "n" => Ok(Token::Sequence(0)),
        "page" => Ok(Token::Page),
        "pages" => Ok(Token::Pages),
        "bookmark" => Ok(Token::Bookmark),
        "text" => Ok(Token::Text),
        other => {
            if let Some(spec) = other.strip_prefix("n:") {
                if !spec.is_empty() && spec.len() <= 9 && spec.chars().all(|c| c == '0') {
                    let width =
                        u8::try_from(spec.len()).map_err(|_| "{n:...} is too wide".to_owned())?;
                    return Ok(Token::Sequence(width));
                }
                return Err(format!("{{{other}}} must look like {{n:000}}"));
            }
            Err(format!("unknown token {{{other}}}"))
        }
    }
}

/// Sejda-style aliases: `[TEXT]`, `[CURRENTPAGE]`, `[BASENAME]`, `[TIMESTAMP]`.
fn match_alias(rest: &str) -> Option<(Vec<Segment>, usize)> {
    const ALIASES: [&str; 4] = ["[TEXT]", "[CURRENTPAGE]", "[BASENAME]", "[TIMESTAMP]"];
    let name = ALIASES.iter().find(|name| rest.starts_with(**name))?;
    let segments = match *name {
        "[TEXT]" => vec![Segment::Token(Token::Text)],
        "[CURRENTPAGE]" => vec![Segment::Token(Token::Page)],
        "[BASENAME]" => vec![Segment::Token(Token::Filename)],
        _ => vec![
            Segment::Token(Token::Date),
            Segment::Literal("_".to_owned()),
            Segment::Token(Token::Time),
        ],
    };
    Some((segments, name.len()))
}

fn token_value(token: Token, ctx: &NameContext<'_>) -> Result<String, DomainError> {
    let missing = |name: &str| {
        DomainError::option(
            "naming pattern",
            format!("{{{name}}} is not available for this tool"),
        )
    };
    Ok(match token {
        Token::Filename => ctx.filename.to_owned(),
        Token::Tool => ctx.tool.to_owned(),
        Token::Date => ctx.now.date(),
        Token::Time => ctx.now.time(),
        Token::Sequence(width) => {
            let n = ctx.sequence.ok_or_else(|| missing("n"))?;
            format!("{n:0>width$}", width = usize::from(width))
        }
        Token::Page => ctx.page.ok_or_else(|| missing("page"))?.to_string(),
        Token::Pages => ctx.pages.ok_or_else(|| missing("pages"))?.to_string(),
        Token::Bookmark => ctx.bookmark.ok_or_else(|| missing("bookmark"))?.to_owned(),
        Token::Text => ctx.text.ok_or_else(|| missing("text"))?.to_owned(),
    })
}

/// Makes a file name stem safe on Windows: strips illegal characters, reserved device names,
/// trailing dots and spaces, and limits the length.
pub fn sanitize_stem(input: &str) -> String {
    let replaced: String = input
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let limited: String = replaced.chars().take(MAX_STEM_CHARS).collect();
    let trimmed = limited.trim_matches(|c: char| c == ' ' || c == '.');
    if trimmed.is_empty() {
        return FALLBACK_STEM.to_owned();
    }
    let device = trimmed.split('.').next().unwrap_or_default().trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(device)) {
        return format!("_{trimmed}");
    }
    trimmed.to_owned()
}

/// Picks a non-colliding file name by appending ` (2)`, ` (3)` and so on.
///
/// `exists` reports whether a candidate full file name is already taken.
pub fn resolve_collision(
    stem: &str,
    extension: &str,
    exists: impl Fn(&str) -> bool,
) -> Result<String, DomainError> {
    let name = |suffix: &str| {
        if extension.is_empty() {
            format!("{stem}{suffix}")
        } else {
            format!("{stem}{suffix}.{extension}")
        }
    };
    let first = name("");
    if !exists(&first) {
        return Ok(first);
    }
    for n in 2..=MAX_COLLISION_ATTEMPTS {
        let candidate = name(&format!(" ({n})"));
        if !exists(&candidate) {
            return Ok(candidate);
        }
    }
    Err(DomainError::option(
        "output name",
        "too many files share this name; choose another folder or pattern",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;

    const NOW: DateTimeParts = DateTimeParts {
        year: 2026,
        month: 10,
        day: 18,
        hour: 9,
        minute: 5,
        second: 7,
    };

    fn ctx() -> NameContext<'static> {
        NameContext {
            filename: "report",
            tool: "compressed",
            now: NOW,
            sequence: Some(7),
            page: Some(3),
            pages: Some(12),
            bookmark: Some("Chapter 1"),
            text: Some("INV-42"),
        }
    }

    fn render(pattern: &str) -> String {
        OutputNamingPattern::parse(pattern)
            .unwrap()
            .render(&ctx())
            .unwrap()
    }

    #[test]
    fn default_pattern_matches_spec_and_parses() {
        assert_eq!(render(DEFAULT_PATTERN), "report (compressed)");
        assert_eq!(
            OutputNamingPattern::default(),
            OutputNamingPattern::parse(DEFAULT_PATTERN).unwrap()
        );
    }

    #[test]
    fn all_tokens_render() {
        assert_eq!(render("{date} {time}"), "2026-10-18 090507");
        assert_eq!(render("{n}-{n:000}"), "7-007");
        assert_eq!(render("{page} of {pages}"), "3 of 12");
        assert_eq!(render("{bookmark} {text}"), "Chapter 1 INV-42");
    }

    #[test]
    fn sejda_aliases_are_accepted() {
        assert_eq!(render("[BASENAME]_[CURRENTPAGE]"), "report_3");
        assert_eq!(render("[TEXT]"), "INV-42");
        assert_eq!(render("[TIMESTAMP]"), "2026-10-18_090507");
    }

    #[test]
    fn bad_patterns_are_rejected() {
        for bad in ["", "{unknown}", "{filename", "filename}", "{n:abc}", "{n:}"] {
            assert!(OutputNamingPattern::parse(bad).is_err(), "pattern {bad:?}");
        }
    }

    #[test]
    fn missing_values_are_reported_not_hidden() {
        let mut c = ctx();
        c.page = None;
        let p = OutputNamingPattern::parse("{page}").unwrap();
        assert!(p.render(&c).is_err());
    }

    #[test]
    fn rendered_values_are_sanitized() {
        let mut c = ctx();
        c.text = Some("a/b:c*d");
        let p = OutputNamingPattern::parse("{text}").unwrap();
        assert_eq!(p.render(&c).unwrap(), "a_b_c_d");
    }

    #[test]
    fn sanitize_handles_reserved_names_dots_spaces_and_length() {
        assert_eq!(sanitize_stem("CON"), "_CON");
        assert_eq!(sanitize_stem("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_stem("com1"), "_com1");
        assert_eq!(sanitize_stem("name. . "), "name");
        assert_eq!(sanitize_stem(".."), "output");
        assert_eq!(sanitize_stem("   "), "output");
        assert_eq!(sanitize_stem("a\u{0}b"), "a_b");
        assert_eq!(sanitize_stem(&"x".repeat(500)).chars().count(), 120);
        assert_eq!(sanitize_stem("COM10"), "COM10");
    }

    #[test]
    fn collisions_add_numbered_suffixes() {
        let taken: HashSet<&str> = ["a.pdf", "a (2).pdf"].into_iter().collect();
        let name = resolve_collision("a", "pdf", |n| taken.contains(n)).unwrap();
        assert_eq!(name, "a (3).pdf");
        assert_eq!(resolve_collision("b", "pdf", |_| false).unwrap(), "b.pdf");
        assert!(resolve_collision("a", "pdf", |_| true).is_err());
    }

    proptest! {
        #[test]
        fn sanitized_stems_are_always_safe(input in ".{0,200}") {
            let s = sanitize_stem(&input);
            prop_assert!(!s.is_empty());
            prop_assert!(s.chars().count() <= 121);
            prop_assert!(!s.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']));
            prop_assert!(!s.ends_with('.') && !s.ends_with(' '));
        }

        #[test]
        fn pattern_parse_never_panics(input in ".{0,64}") {
            let _ = OutputNamingPattern::parse(&input);
        }
    }
}
