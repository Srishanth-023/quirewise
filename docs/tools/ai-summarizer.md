# AI Summarizer (Appendix A row 56)

## Purpose

Writes a summary of a PDF with a language model on this computer, in the language you choose, as a PDF or a Markdown file. The original is never changed. Nothing is sent anywhere else.

## Options

| Option | Type | Meaning |
|---|---|---|
| `language` | text | Language of the summary, as a code such as `de` (about 36 are offered); default `en`. The desktop app offers the languages in a menu; the interface is English for now, so English is the default. |
| `length` | choice | `short` (one paragraph), `medium` (about three paragraphs, default) or `long` (headings and bullet points) |
| `format` | choice | `pdf` (default) or `markdown` |
| `pages` | text | Pages to summarize; default `all` |
| `ai` | group | The model; see `smart-split.md`, "The model" |

```
qw run ai-summarizer report.pdf --options '{"language":"de","length":"short","format":"markdown"}'
```

The result is named `{name} (summary).pdf` or `.md`.

## How it works

1. The text of the chosen pages is read and cut into pieces of at most 5000 characters, at paragraph or sentence ends.
2. One piece goes in one request that writes the final summary. More pieces are summarized one by one (3 to 6 sentences each); if those summaries do not fit one request they are summarized again, up to four rounds; then one request writes the final summary from them in the chosen length and language.
3. The model writes plain Markdown. A Markdown file gets the name of the document as its title. A PDF is set as flowing text on pages of the size of the first page (portrait): title, headings, paragraphs and bullet points.
4. The PDF is set in a standard font when the text fits it, otherwise in an installed TrueType font that has every character (Segoe UI, Arial, Calibri and others are tried first), embedded with a map back to the characters so the text can be searched and copied. If no installed font has the characters the tool says so.

## Behavior and limits

- The model is told to use only what the text says. A small model may still get things wrong: a summary supports reading the document, it does not replace it.
- The text of the pages is data: the prompts tell the model never to follow sentences inside it.
- Languages written right to left, and Indic and Thai scripts, are not drawn in a PDF yet (no text shaping); ask for Markdown for them. Chinese and Japanese need an installed TrueType font that has them (many Windows fonts for these languages are collections, which are not used yet).
- Scanned pages have no text: run OCR first.

## Errors

`QW-OPTION-INVALID` (language not offered, a language that a PDF cannot show, no text on the pages), `QW-NETWORK-CONSENT-REQUIRED`, `QW-COMPONENT-MISSING` (also when no installed font has the characters), `QW-NETWORK-UNAVAILABLE`, `QW-OUTPUT-VERIFY-FAILED`, `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`, and the input errors of every PDF tool.

## License impact

As for Smart Split. The embedded fonts are the ones installed on the computer; a font that does not allow embedding is refused.
