# Translate PDF (Appendix A row 57)

## Purpose

Translates the text of a PDF with a language model on this computer. With the layout kept, the pages stay as they are and the translation is written where the text was. The original is never changed. Nothing is sent anywhere else.

## Options

| Option | Type | Meaning |
|---|---|---|
| `target_language` | text | Language to translate into, as a code such as `de`; default `en`. Languages that a PDF cannot show are refused. |
| `preserve_layout` | bool | Default on: keep the pages and write the translation in the blocks of text. Off: write a new document of flowing text. |
| `pages` | text | Pages to translate; default `all`. Other pages are kept as they are when the layout is kept; a new document holds only the chosen pages. |
| `ai` | group | The model; see `smart-split.md`, "The model" |

```
qw run translate-pdf letter.pdf --options '{"target_language":"pl"}'
```

The result is named `{name} (translated {code}).pdf`.

## How it works

1. Each page is read as blocks of text (headings, paragraphs, list items and the cells of tables) with their places, sizes and weights, from the same layout analysis as the editor and PDF to Word.
2. The blocks of a page are sent to the model together, as JSON with an id for each, in batches of about 2500 characters. Texts that appear again (running heads) are asked once; texts without a letter (numbers, signs) are not asked. Numbers, dates, addresses, product codes and names of people and companies are to stay as they are.
3. An answer is matched to its block by id. A text that is missing, empty or implausibly long (more than five times the original) is asked for again alone; if that fails too the block keeps its original text. When more than half of the texts fail, nothing is written.
4. **Layout kept:** the old text of each translated block is removed from the page itself (the same engine as Redact, with white boxes), then the translation is written in the block at the original size, shrunk down to 5 points until it fits the box, in one font for the whole document: Helvetica when the text is WinAnsi, otherwise an installed TrueType font that has every character (Segoe UI, Arial, Calibri and others are tried first), embedded so the text can be searched and copied. A white patch is drawn under the text, so a page that the cleaner cannot clean still looks right. Pictures, lines and everything that is not text stay.
5. **Layout not kept:** headings, paragraphs and list items (table cells as list items) are set as flowing text on pages the size of the first page.

## Behavior and limits

- A translation is longer than its original in many languages; the text is shrunk to fit its block, down to 5 points, and runs past the block after that.
- Text that is part of a picture is not translated. Scanned pages have no text: run OCR first, then translate the result.
- Right to left, Indic and Thai scripts are not drawn (no text shaping). Chinese and Japanese need an installed TrueType font that has them.
- A block is the unit: text whose blocks the layout analysis got wrong comes out as wrong as the blocks.
- Signatures made earlier stop validating, as with every tool that rewrites the file. Passwords are kept: a protected file gives a protected result.
- A model translates well in proportion to its size; read the result before you send it on.

## Errors

`QW-OPTION-INVALID`, `QW-NETWORK-CONSENT-REQUIRED`, `QW-COMPONENT-MISSING`, `QW-NETWORK-UNAVAILABLE`, `QW-OUTPUT-VERIFY-FAILED` (also when the model could not translate most of the text), `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`, and the input errors of every PDF tool.

## License impact

As for Smart Split.
