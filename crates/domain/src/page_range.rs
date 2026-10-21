//! Page range grammar (docs/project/MASTER_PROMPT.md Appendix C.1) and resolution against a page count.
//!
//! Parsing is independent of the document so it can be validated as the user types.
//! Resolution applies the page count and the duplicate policy.

use core::fmt;
use core::num::NonZeroU32;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::units::PageIndex;

/// A page position that may be counted from the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// A one-based page number.
    Number(NonZeroU32),
    /// `last` (0) or `last-K`, counted back from the final page.
    FromEnd(u32),
}

/// One comma-separated element of a range expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeItem {
    /// `all`
    All,
    /// `even`
    Even,
    /// `odd`
    Odd,
    /// A single page, for example `5` or `last-2`.
    Single(Endpoint),
    /// `3-8`, `10-` (open end) or `-3` (from page 1).
    Span {
        /// First page.
        from: Endpoint,
        /// Last page, or `None` to run to the end.
        to: Option<Endpoint>,
    },
}

/// Whether the same page may be selected more than once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duplicates {
    /// Allowed (merge, organize, alternate and mix).
    Allow,
    /// Rejected with a syntax error naming the repeated page.
    Reject,
}

/// A parsed page range expression such as `1,3,5-8,last`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
#[schemars(with = "String")]
pub struct PageRange {
    items: Vec<RangeItem>,
}

impl PageRange {
    /// The range `all`.
    pub fn all() -> Self {
        Self {
            items: vec![RangeItem::All],
        }
    }

    /// Builds a range from already parsed items, for example one element of a split list.
    pub fn from_items(items: Vec<RangeItem>) -> Self {
        Self { items }
    }

    /// The parsed elements, in the order written.
    pub fn items(&self) -> &[RangeItem] {
        &self.items
    }

    /// Parses an expression. Whitespace is ignored and keywords are case-insensitive.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let syntax = |reason: &str| DomainError::RangeSyntax {
            input: input.to_owned(),
            reason: reason.to_owned(),
        };
        let tokens = tokenize(input).map_err(|r| syntax(&r))?;
        if tokens.is_empty() {
            return Err(syntax("the range is empty"));
        }
        let mut items = Vec::new();
        for segment in tokens.split(|t| *t == Token::Comma) {
            items.push(parse_segment(segment).map_err(|r| syntax(&r))?);
        }
        Ok(Self { items })
    }

    /// Resolves the range against `page_count`, returning pages in the order listed.
    pub fn resolve(
        &self,
        page_count: u32,
        duplicates: Duplicates,
    ) -> Result<PageSelection, DomainError> {
        if page_count == 0 {
            return Err(DomainError::RangeOutOfBounds {
                detail: "This document has no pages.".to_owned(),
                valid: "none".to_owned(),
            });
        }
        let valid = if page_count == 1 {
            "1".to_owned()
        } else {
            format!("1-{page_count}")
        };
        let out_of_bounds = |detail: String| DomainError::RangeOutOfBounds {
            detail,
            valid: valid.clone(),
        };
        let eval = |e: Endpoint| -> i64 {
            match e {
                Endpoint::Number(n) => i64::from(n.get()),
                Endpoint::FromEnd(k) => i64::from(page_count) - i64::from(k),
            }
        };
        let check = |n: i64| -> Result<u32, DomainError> {
            if n < 1 || n > i64::from(page_count) {
                Err(out_of_bounds(format!("Page {n} is outside this document.")))
            } else {
                u32::try_from(n - 1)
                    .map_err(|_| out_of_bounds(format!("Page {n} is outside this document.")))
            }
        };

        let mut pages = Vec::new();
        for item in &self.items {
            match *item {
                RangeItem::All => pages.extend(0..page_count),
                RangeItem::Even => pages.extend((1..page_count).step_by(2)),
                RangeItem::Odd => pages.extend((0..page_count).step_by(2)),
                RangeItem::Single(e) => pages.push(check(eval(e))?),
                RangeItem::Span { from, to } => {
                    let start = eval(from);
                    let end = to.map_or(i64::from(page_count), eval);
                    if start > end {
                        return Err(DomainError::RangeSyntax {
                            input: self.to_string(),
                            reason: "a range starts after it ends".to_owned(),
                        });
                    }
                    let (first, last) = (check(start)?, check(end)?);
                    pages.extend(first..=last);
                }
            }
        }

        if duplicates == Duplicates::Reject {
            let mut seen = vec![false; page_count as usize];
            for &p in &pages {
                if let Some(slot) = seen.get_mut(p as usize) {
                    if *slot {
                        return Err(DomainError::RangeSyntax {
                            input: self.to_string(),
                            reason: format!("page {} is listed more than once", p + 1),
                        });
                    }
                    *slot = true;
                }
            }
        }
        Ok(PageSelection {
            pages: pages.into_iter().map(PageIndex::new).collect(),
        })
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::FromEnd(0) => f.write_str("last"),
            Self::FromEnd(k) => write!(f, "last-{k}"),
        }
    }
}

impl fmt::Display for PageRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            match item {
                RangeItem::All => f.write_str("all")?,
                RangeItem::Even => f.write_str("even")?,
                RangeItem::Odd => f.write_str("odd")?,
                RangeItem::Single(e) => write!(f, "{e}")?,
                RangeItem::Span { from, to: Some(to) } => write!(f, "{from}-{to}")?,
                RangeItem::Span { from, to: None } => write!(f, "{from}-")?,
            }
        }
        Ok(())
    }
}

impl TryFrom<String> for PageRange {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<PageRange> for String {
    fn from(value: PageRange) -> Self {
        value.to_string()
    }
}

impl Default for PageRange {
    fn default() -> Self {
        Self::all()
    }
}

/// Pages chosen by a range, as zero-based indices in the order listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSelection {
    pages: Vec<PageIndex>,
}

impl PageSelection {
    /// The selected pages.
    pub fn pages(&self) -> &[PageIndex] {
        &self.pages
    }

    /// Number of selected pages.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// True when nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// True when `page` is part of the selection.
    pub fn contains(&self, page: PageIndex) -> bool {
        self.pages.contains(&page)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Number(u32),
    Word(String),
    Dash,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().filter(|c| !c.is_whitespace()).peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut value: u32 = 0;
            while let Some(&d) = chars.peek() {
                let Some(digit) = d.to_digit(10).filter(|_| d.is_ascii_digit()) else {
                    break;
                };
                value = value
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(digit))
                    .ok_or_else(|| "a page number is too large".to_owned())?;
                chars.next();
            }
            tokens.push(Token::Number(value));
        } else if c.is_ascii_alphabetic() {
            let mut word = String::new();
            while let Some(&a) = chars.peek() {
                if !a.is_ascii_alphabetic() {
                    break;
                }
                word.push(a.to_ascii_lowercase());
                chars.next();
            }
            tokens.push(Token::Word(word));
        } else if c == '-' {
            tokens.push(Token::Dash);
            chars.next();
        } else if c == ',' {
            tokens.push(Token::Comma);
            chars.next();
        } else {
            return Err(format!("unexpected character \"{c}\""));
        }
    }
    Ok(tokens)
}

/// Parses `NUMBER | last [- NUMBER]`, returning the endpoint and the tokens consumed.
fn parse_endpoint(tokens: &[Token]) -> Result<(Endpoint, usize), String> {
    match tokens.first() {
        Some(Token::Number(0)) => Err("page numbers start at 1".to_owned()),
        Some(Token::Number(n)) => NonZeroU32::new(*n)
            .map(|n| (Endpoint::Number(n), 1))
            .ok_or_else(|| "page numbers start at 1".to_owned()),
        Some(Token::Word(w)) if w == "last" => {
            if let (Some(Token::Dash), Some(Token::Number(k))) = (tokens.get(1), tokens.get(2)) {
                Ok((Endpoint::FromEnd(*k), 3))
            } else {
                Ok((Endpoint::FromEnd(0), 1))
            }
        }
        Some(Token::Word(w)) => Err(format!("unknown word \"{w}\"")),
        Some(Token::Dash) => Err("a dash needs a page number after it".to_owned()),
        Some(Token::Comma) | None => Err("a page is missing".to_owned()),
    }
}

fn parse_segment(segment: &[Token]) -> Result<RangeItem, String> {
    if segment.is_empty() {
        return Err("a page is missing between commas".to_owned());
    }
    if let [Token::Word(w)] = segment {
        match w.as_str() {
            "all" => return Ok(RangeItem::All),
            "even" => return Ok(RangeItem::Even),
            "odd" => return Ok(RangeItem::Odd),
            _ => {}
        }
    }
    // `-3` means pages 1 to 3.
    if let [Token::Dash, Token::Number(n)] = segment {
        let end = NonZeroU32::new(*n).ok_or_else(|| "page numbers start at 1".to_owned())?;
        return Ok(RangeItem::Span {
            from: Endpoint::Number(NonZeroU32::MIN),
            to: Some(Endpoint::Number(end)),
        });
    }
    let (from, used) = parse_endpoint(segment)?;
    let rest = segment.get(used..).unwrap_or_default();
    match rest {
        [] => Ok(RangeItem::Single(from)),
        [Token::Dash] => Ok(RangeItem::Span { from, to: None }),
        [Token::Dash, tail @ ..] => {
            let (to, used_to) = parse_endpoint(tail)?;
            if used_to == tail.len() {
                Ok(RangeItem::Span { from, to: Some(to) })
            } else {
                Err("unexpected text after the end of a range".to_owned())
            }
        }
        _ => Err("pages must be separated by commas".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use proptest::prelude::*;

    fn resolve(expr: &str, count: u32) -> Result<Vec<u32>, DomainError> {
        PageRange::parse(expr)?
            .resolve(count, Duplicates::Allow)
            .map(|s| s.pages().iter().map(|p| p.get() + 1).collect())
    }

    #[test]
    fn grammar_examples_from_the_spec() {
        assert_eq!(resolve("5", 10).unwrap(), vec![5]);
        assert_eq!(resolve("3-8", 10).unwrap(), vec![3, 4, 5, 6, 7, 8]);
        assert_eq!(resolve("10-", 12).unwrap(), vec![10, 11, 12]);
        assert_eq!(resolve("-3", 10).unwrap(), vec![1, 2, 3]);
        assert_eq!(resolve("last", 10).unwrap(), vec![10]);
        assert_eq!(resolve("last-2", 10).unwrap(), vec![8]);
        assert_eq!(resolve("even", 6).unwrap(), vec![2, 4, 6]);
        assert_eq!(resolve("odd", 6).unwrap(), vec![1, 3, 5]);
        assert_eq!(resolve("all", 3).unwrap(), vec![1, 2, 3]);
        assert_eq!(
            resolve("1,3,5-8,last", 10).unwrap(),
            vec![1, 3, 5, 6, 7, 8, 10]
        );
    }

    #[test]
    fn whitespace_and_case_are_ignored() {
        assert_eq!(resolve(" 1 , 3 - 4 , LAST ", 5).unwrap(), vec![1, 3, 4, 5]);
        assert_eq!(resolve("1 2", 50).unwrap(), vec![12]);
    }

    #[test]
    fn ranges_may_end_at_last_expressions() {
        assert_eq!(resolve("3-last", 6).unwrap(), vec![3, 4, 5, 6]);
        assert_eq!(resolve("3-last-1", 6).unwrap(), vec![3, 4, 5]);
    }

    #[test]
    fn rejects_zero_reversed_and_garbage() {
        for bad in [
            "0", "0-3", "5-3", "", ",", "1,,2", "abc", "1-2-3", "1;2", "-", "--3",
        ] {
            let err = PageRange::parse(bad).and_then(|r| r.resolve(10, Duplicates::Allow));
            assert_eq!(
                err.unwrap_err().code(),
                ErrorCode::RangeSyntax,
                "input {bad:?}"
            );
        }
    }

    #[test]
    fn out_of_bounds_reports_the_valid_range() {
        let err = resolve("11", 10).unwrap_err();
        assert_eq!(err.code(), ErrorCode::RangeOutOfBounds);
        assert!(err.to_string().contains("1-10"));
        assert_eq!(
            resolve("last-10", 10).unwrap_err().code(),
            ErrorCode::RangeOutOfBounds
        );
        assert_eq!(
            resolve("8-12", 10).unwrap_err().code(),
            ErrorCode::RangeOutOfBounds
        );
    }

    #[test]
    fn zero_page_documents_are_rejected() {
        assert_eq!(
            resolve("all", 0).unwrap_err().code(),
            ErrorCode::RangeOutOfBounds
        );
    }

    #[test]
    fn duplicates_follow_the_policy() {
        let r = PageRange::parse("1,1-2").unwrap();
        assert_eq!(r.resolve(5, Duplicates::Allow).unwrap().len(), 3);
        assert_eq!(
            r.resolve(5, Duplicates::Reject).unwrap_err().code(),
            ErrorCode::RangeSyntax
        );
    }

    #[test]
    fn display_round_trips() {
        for expr in ["all", "1,3,5-8,last", "10-", "last-2", "3-last-1", "even"] {
            let parsed = PageRange::parse(expr).unwrap();
            assert_eq!(parsed.to_string(), expr);
        }
        assert_eq!(PageRange::parse("-3").unwrap().to_string(), "1-3");
    }

    #[test]
    fn serde_uses_the_string_form() {
        let r: PageRange = serde_json::from_str("\"2-4\"").unwrap();
        assert_eq!(serde_json::to_string(&r).unwrap(), "\"2-4\"");
        assert!(serde_json::from_str::<PageRange>("\"0\"").is_err());
    }

    proptest! {
        #[test]
        fn parse_never_panics(input in ".{0,64}") {
            let _ = PageRange::parse(&input);
        }

        #[test]
        fn resolved_pages_are_always_in_bounds(
            a in 1u32..200, b in 1u32..200, count in 1u32..200,
        ) {
            let expr = format!("{a}-{b}");
            if let Ok(sel) = PageRange::parse(&expr).and_then(|r| r.resolve(count, Duplicates::Allow)) {
                prop_assert!(sel.pages().iter().all(|p| p.get() < count));
            }
        }
    }
}
