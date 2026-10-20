//! Options of the tools that use a language model on this computer: Smart Split (row 12), AI
//! Summarizer (row 56) and Translate PDF (row 57), and the AI parts of Redact (row 54) and PDF to
//! Markdown (row 29).
//!
//! The model runs on this computer and nothing is sent anywhere else. Two ways to run it:
//! `bundled`, the llama.cpp server that ships as a component with a model file the person
//! downloaded, and `ollama`, an Ollama server that already runs on this computer, used only after
//! the person agreed to it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::page_range::PageRange;

/// Longest instruction given to Smart Split, in characters.
pub const MAX_INSTRUCTION_CHARS: usize = 2_000;
/// Longest title of a part, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Most parts one Smart Split may make.
pub const MAX_PARTS: usize = 500;
/// The address Ollama listens on unless told otherwise.
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";
/// The model used when none is chosen.
pub const DEFAULT_MODEL: &str = "qwen2.5-1.5b-instruct";

/// Where the language model runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AiBackend {
    /// The llama.cpp server that comes as a component, with a model file from the Components
    /// screen (default).
    #[default]
    Bundled,
    /// An Ollama server on this computer.
    Ollama,
}

/// Which model runs and where. Shared by every tool that uses the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AiOptions {
    /// Where the model runs. Default `bundled`.
    pub backend: AiBackend,
    /// The model. For `bundled`, the id of a model of the catalog (default
    /// `qwen2.5-1.5b-instruct`). For `ollama`, the name of an installed model such as
    /// `llama3.2:3b`; none uses the first one the server lists.
    pub model: Option<String>,
    /// The address of the Ollama server. Only this computer is allowed: `127.0.0.1`, `localhost`
    /// or `[::1]`. Default `http://127.0.0.1:11434`.
    pub ollama_url: String,
    /// Agree to talk to the Ollama server on this computer. Needed for `ollama`. Default off.
    pub allow_ollama: bool,
}

impl Default for AiOptions {
    fn default() -> Self {
        Self {
            backend: AiBackend::Bundled,
            model: None,
            ollama_url: DEFAULT_OLLAMA_URL.to_owned(),
            allow_ollama: false,
        }
    }
}

/// True for the characters a model name may hold.
fn model_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-')
}

/// The host and port of a loopback address such as `http://127.0.0.1:11434`, or `None` when the
/// address is anything else: another host, a path, a user name, a secure scheme.
pub fn loopback_endpoint(url: &str) -> Option<(String, u16)> {
    let rest = url.trim().strip_prefix("http://")?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty()
        || rest
            .chars()
            .any(|c| matches!(c, '/' | '?' | '#' | '@' | '\\'))
    {
        return None;
    }
    let (host, port) = if let Some(inner) = rest.strip_prefix('[') {
        let (host, tail) = inner.split_once(']')?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p,
            None if tail.is_empty() => "80",
            None => return None,
        };
        (format!("[{host}]"), port)
    } else {
        match rest.split_once(':') {
            Some((host, port)) => (host.to_owned(), port),
            None => (rest.to_owned(), "80"),
        }
    };
    let port: u16 = port.parse().ok().filter(|p| *p != 0)?;
    let lower = host.to_ascii_lowercase();
    matches!(lower.as_str(), "127.0.0.1" | "localhost" | "[::1]").then_some((lower, port))
}

impl AiOptions {
    /// Checks the model name and, for Ollama, the address and the agreement.
    pub fn validate(&self) -> Result<(), DomainError> {
        if let Some(model) = &self.model
            && (model.is_empty() || model.len() > 100 || !model.chars().all(model_char))
        {
            return Err(DomainError::option(
                "model",
                "use the name of a model: letters, digits and . _ : / -",
            ));
        }
        if self.backend == AiBackend::Ollama {
            if !self.allow_ollama {
                return Err(DomainError::LocalConsentNeeded {
                    what: "Using the Ollama server on this computer".into(),
                    option: "Allow Ollama on this computer".into(),
                });
            }
            if loopback_endpoint(&self.ollama_url).is_none() {
                return Err(DomainError::option(
                    "ollama_url",
                    "only this computer is allowed: use an address such as http://127.0.0.1:11434",
                ));
            }
        } else if let Some(model) = &self.model
            && crate::tools::ai::model(model).is_none()
        {
            return Err(DomainError::option(
                "model",
                format!("{model} is not a model of the catalog"),
            ));
        }
        Ok(())
    }

    /// The id of the model to run for `bundled`.
    pub fn bundled_model(&self) -> &str {
        self.model.as_deref().unwrap_or(DEFAULT_MODEL)
    }
}

/// A model of the catalog: what the Components screen shows before it downloads the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AiModel {
    /// The id used in options and as the component id suffix: `model-<id>`.
    pub id: &'static str,
    /// The name for people.
    pub name: &'static str,
    /// The license of the model's weights.
    pub license: &'static str,
    /// Where the license text is.
    pub license_url: &'static str,
    /// What the license allows, in a sentence or two.
    pub license_summary: &'static str,
    /// The file name inside the component folder.
    pub file: &'static str,
    /// Download size in megabytes, about.
    pub size_mb: u32,
    /// What the model is good for.
    pub note: &'static str,
}

/// The models Quirewise can download. Every entry is under a license that lets the person use it
/// for their own work, including at a company. The weights are never part of the installer.
pub const MODELS: [AiModel; 3] = [
    AiModel {
        id: "smollm2-360m-instruct",
        name: "SmolLM2 360M Instruct",
        license: "Apache-2.0",
        license_url: "https://www.apache.org/licenses/LICENSE-2.0",
        license_summary: "Free to use, copy and change, also for work, with a notice of the license kept.",
        file: "smollm2-360m-instruct-q8_0.gguf",
        size_mb: 386,
        note: "Very small and fast. English only in practice. For trying the tools on a slow computer.",
    },
    AiModel {
        id: "qwen2.5-1.5b-instruct",
        name: "Qwen2.5 1.5B Instruct",
        license: "Apache-2.0",
        license_url: "https://www.apache.org/licenses/LICENSE-2.0",
        license_summary: "Free to use, copy and change, also for work, with a notice of the license kept.",
        file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        size_mb: 1_120,
        note: "Default. Works in about 30 languages and runs on a computer without a graphics card.",
    },
    AiModel {
        id: "qwen2.5-7b-instruct",
        name: "Qwen2.5 7B Instruct",
        license: "Apache-2.0",
        license_url: "https://www.apache.org/licenses/LICENSE-2.0",
        license_summary: "Free to use, copy and change, also for work, with a notice of the license kept.",
        file: "qwen2.5-7b-instruct-q4_k_m.gguf",
        size_mb: 4_680,
        note: "Better summaries and translations. Needs about 8 GB of free memory and is slow without a graphics card.",
    },
];

/// The model of the catalog with this id.
pub fn model(id: &str) -> Option<&'static AiModel> {
    MODELS.iter().find(|m| m.id == id)
}

/// A language the tools can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Language {
    /// The code, such as `de`.
    pub code: &'static str,
    /// The name in English, used in the instructions to the model.
    pub name: &'static str,
    /// True when a PDF can show it: the script needs no shaping and an installed font covers it.
    /// Languages written right to left, and Indic and Thai scripts, are not drawn.
    pub drawable: bool,
}

const fn lang(code: &'static str, name: &'static str, drawable: bool) -> Language {
    Language {
        code,
        name,
        drawable,
    }
}

/// The languages the summarizer and the translator write.
pub const LANGUAGES: [Language; 36] = [
    lang("en", "English", true),
    lang("de", "German", true),
    lang("fr", "French", true),
    lang("es", "Spanish", true),
    lang("it", "Italian", true),
    lang("pt", "Portuguese", true),
    lang("nl", "Dutch", true),
    lang("pl", "Polish", true),
    lang("cs", "Czech", true),
    lang("sk", "Slovak", true),
    lang("sl", "Slovenian", true),
    lang("hr", "Croatian", true),
    lang("hu", "Hungarian", true),
    lang("ro", "Romanian", true),
    lang("sv", "Swedish", true),
    lang("da", "Danish", true),
    lang("no", "Norwegian", true),
    lang("fi", "Finnish", true),
    lang("et", "Estonian", true),
    lang("lv", "Latvian", true),
    lang("lt", "Lithuanian", true),
    lang("tr", "Turkish", true),
    lang("id", "Indonesian", true),
    lang("vi", "Vietnamese", true),
    lang("ru", "Russian", true),
    lang("uk", "Ukrainian", true),
    lang("bg", "Bulgarian", true),
    lang("el", "Greek", true),
    lang("zh", "Chinese", true),
    lang("ja", "Japanese", true),
    lang("ko", "Korean", true),
    lang("ar", "Arabic", false),
    lang("he", "Hebrew", false),
    lang("hi", "Hindi", false),
    lang("ta", "Tamil", false),
    lang("th", "Thai", false),
];

/// The language with this code.
pub fn language(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code))
}

fn check_language(field: &str, code: &str) -> Result<&'static Language, DomainError> {
    language(code).ok_or_else(|| {
        DomainError::option(
            field,
            format!("{code} is not a language that is offered; use a code such as en, de or fr"),
        )
    })
}

/// How long a summary is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SummaryLength {
    /// A short paragraph.
    Short,
    /// A page or so (default).
    #[default]
    Medium,
    /// Several paragraphs with headings.
    Long,
}

/// What a summary is saved as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SummaryFormat {
    /// A PDF (default).
    #[default]
    Pdf,
    /// A Markdown file.
    Markdown,
}

impl SummaryFormat {
    /// The file extension.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Markdown => "md",
        }
    }
}

/// One part of a document, made by Smart Split.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitPart {
    /// The first page, from 1.
    pub start: u32,
    /// The last page, from 1.
    pub end: u32,
    /// What the part is about; used for the name of the file.
    pub title: String,
}

/// Options for Smart Split.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SmartSplitOptions {
    /// The model.
    pub ai: AiOptions,
    /// What to split by, in your own words, such as "one file for each invoice".
    pub instruction: String,
    /// The parts to make. Empty: the model proposes them from the instruction. The preview of
    /// the desktop app fills this in, so what you checked is what is made.
    pub parts: Vec<SplitPart>,
}

impl SmartSplitOptions {
    /// Checks the options against a document of `count` pages.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        self.ai.validate()?;
        if self.parts.is_empty() {
            return self.check_instruction();
        }
        self.check_parts(count)
    }

    /// An instruction of one to 2000 characters is needed to ask the model.
    pub fn check_instruction(&self) -> Result<(), DomainError> {
        let n = self.instruction.trim().chars().count();
        if n == 0 {
            return Err(DomainError::option(
                "instruction",
                "say how to split, for example: one file for each invoice",
            ));
        }
        if self.instruction.chars().count() > MAX_INSTRUCTION_CHARS {
            return Err(DomainError::option(
                "instruction",
                format!("use at most {MAX_INSTRUCTION_CHARS} characters"),
            ));
        }
        Ok(())
    }

    /// The parts must be in page order, must not overlap and must lie inside the document.
    pub fn check_parts(&self, count: u32) -> Result<(), DomainError> {
        check_parts(&self.parts, count)
    }
}

/// Checks parts against a document of `count` pages: in page order, not overlapping, inside it.
pub fn check_parts(parts: &[SplitPart], count: u32) -> Result<(), DomainError> {
    if parts.len() > MAX_PARTS {
        return Err(DomainError::option("parts", "too many parts"));
    }
    let mut next = 1;
    for part in parts {
        if part.start == 0 || part.end < part.start || part.end > count {
            return Err(DomainError::option(
                "parts",
                format!(
                    "pages {} to {} are not inside this document of {count} pages",
                    part.start, part.end
                ),
            ));
        }
        if part.start < next {
            return Err(DomainError::option(
                "parts",
                "parts must be in page order and must not overlap",
            ));
        }
        if part.title.chars().count() > MAX_TITLE_CHARS || part.title.chars().any(char::is_control)
        {
            return Err(DomainError::option(
                "parts",
                "a title is too long or has control characters",
            ));
        }
        next = part.end + 1;
    }
    Ok(())
}

/// Options for AI Summarizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AiSummarizerOptions {
    /// The model.
    pub ai: AiOptions,
    /// The language of the summary, as a code such as `de`. Default `en`; the desktop app sets
    /// the language of the interface.
    pub language: String,
    /// How long the summary is. Default medium.
    pub length: SummaryLength,
    /// PDF or Markdown. Default PDF.
    pub format: SummaryFormat,
    /// Pages to summarize. Default `all`.
    pub pages: PageRange,
}

impl Default for AiSummarizerOptions {
    fn default() -> Self {
        Self {
            ai: AiOptions::default(),
            language: "en".to_owned(),
            length: SummaryLength::Medium,
            format: SummaryFormat::Pdf,
            pages: PageRange::all(),
        }
    }
}

impl AiSummarizerOptions {
    /// Checks the model and the language. A PDF can only hold languages that can be drawn.
    pub fn validate(&self) -> Result<&'static Language, DomainError> {
        self.ai.validate()?;
        let lang = check_language("language", &self.language)?;
        if self.format == SummaryFormat::Pdf && !lang.drawable {
            return Err(DomainError::option(
                "format",
                format!(
                    "a PDF cannot show {} yet; choose Markdown for this language",
                    lang.name
                ),
            ));
        }
        Ok(lang)
    }
}

/// Options for Translate PDF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TranslatePdfOptions {
    /// The model.
    pub ai: AiOptions,
    /// The language to translate into, as a code such as `de`. Default `en`.
    pub target_language: String,
    /// Keep the pages as they are and put the translation where the text was. Off writes a new
    /// document of flowing text. Default on.
    pub preserve_layout: bool,
    /// Pages to translate. Default `all`.
    pub pages: PageRange,
}

impl Default for TranslatePdfOptions {
    fn default() -> Self {
        Self {
            ai: AiOptions::default(),
            target_language: "en".to_owned(),
            preserve_layout: true,
            pages: PageRange::all(),
        }
    }
}

impl TranslatePdfOptions {
    /// Checks the model and the language. The result is a PDF, so it must be drawable.
    pub fn validate(&self) -> Result<&'static Language, DomainError> {
        self.ai.validate()?;
        let lang = check_language("target_language", &self.target_language)?;
        if !lang.drawable {
            return Err(DomainError::option(
                "target_language",
                format!(
                    "a PDF cannot show {} yet (right to left, Indic and Thai scripts need shaping); choose another language",
                    lang.name
                ),
            ));
        }
        Ok(lang)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_computer_is_a_valid_ollama_address() {
        for ok in [
            "http://127.0.0.1:11434",
            "http://localhost:11434/",
            "http://[::1]:8080",
            "http://LOCALHOST",
        ] {
            assert!(loopback_endpoint(ok).is_some(), "{ok}");
        }
        for bad in [
            "https://127.0.0.1:11434",
            "http://192.168.1.5:11434",
            "http://example.com",
            "http://127.0.0.1.evil.com:80",
            "http://127.0.0.1:11434/v1",
            "http://user@127.0.0.1:11434",
            "http://127.0.0.1:0",
            "http://127.0.0.1:99999",
            "127.0.0.1:11434",
            "http://",
            "",
        ] {
            assert!(loopback_endpoint(bad).is_none(), "{bad}");
        }
        assert_eq!(
            loopback_endpoint("http://localhost:11434"),
            Some(("localhost".to_owned(), 11434))
        );
    }

    #[test]
    fn ollama_needs_agreement_and_a_local_address() {
        let mut ai = AiOptions {
            backend: AiBackend::Ollama,
            ..AiOptions::default()
        };
        assert!(matches!(
            ai.validate(),
            Err(DomainError::LocalConsentNeeded { .. })
        ));
        ai.allow_ollama = true;
        assert!(ai.validate().is_ok());
        ai.ollama_url = "http://10.0.0.5:11434".into();
        assert!(matches!(
            ai.validate(),
            Err(DomainError::OptionInvalid { .. })
        ));
    }

    #[test]
    fn bundled_models_come_from_the_catalog() {
        let ok = AiOptions {
            model: Some("qwen2.5-7b-instruct".into()),
            ..AiOptions::default()
        };
        assert!(ok.validate().is_ok());
        let bad = AiOptions {
            model: Some("gpt-9".into()),
            ..AiOptions::default()
        };
        assert!(bad.validate().is_err());
        assert_eq!(AiOptions::default().bundled_model(), DEFAULT_MODEL);
        assert!(model(DEFAULT_MODEL).is_some());
        let odd = AiOptions {
            model: Some("a b".into()),
            ..AiOptions::default()
        };
        assert!(odd.validate().is_err());
    }

    #[test]
    fn catalog_ids_and_language_codes_are_unique() {
        for (i, m) in MODELS.iter().enumerate() {
            assert!(MODELS[..i].iter().all(|o| o.id != m.id));
            assert!(
                m.id.chars().all(|c| c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || matches!(c, '-' | '.'))
            );
            assert!(!m.license.is_empty() && m.license_url.starts_with("https://"));
        }
        for (i, l) in LANGUAGES.iter().enumerate() {
            assert!(LANGUAGES[..i].iter().all(|o| o.code != l.code));
        }
    }

    #[test]
    fn a_pdf_cannot_hold_a_language_that_needs_shaping() {
        let mut s = AiSummarizerOptions {
            language: "ar".into(),
            ..AiSummarizerOptions::default()
        };
        assert!(s.validate().is_err());
        s.format = SummaryFormat::Markdown;
        assert!(s.validate().is_ok());
        s.language = "xx".into();
        assert!(s.validate().is_err());
        let t = TranslatePdfOptions {
            target_language: "hi".into(),
            ..TranslatePdfOptions::default()
        };
        assert!(t.validate().is_err());
        assert!(TranslatePdfOptions::default().validate().is_ok());
    }

    fn part(start: u32, end: u32) -> SplitPart {
        SplitPart {
            start,
            end,
            title: format!("Part {start}"),
        }
    }

    #[test]
    fn smart_split_needs_an_instruction_or_checked_parts() {
        let mut o = SmartSplitOptions::default();
        assert!(o.validate(10).is_err());
        o.instruction = "one file for each invoice".into();
        assert!(o.validate(10).is_ok());
        o.parts = vec![part(1, 3), part(4, 10)];
        assert!(o.validate(10).is_ok());
        o.parts = vec![part(1, 3), part(3, 10)];
        assert!(o.validate(10).is_err(), "overlap");
        o.parts = vec![part(4, 10), part(1, 3)];
        assert!(o.validate(10).is_err(), "order");
        o.parts = vec![part(1, 11)];
        assert!(o.validate(10).is_err(), "outside");
        o.parts = vec![part(0, 2)];
        assert!(o.validate(10).is_err(), "page zero");
        o.parts = vec![part(1, 2), part(5, 6)];
        assert!(o.validate(10).is_ok(), "gaps are allowed");
    }
}
