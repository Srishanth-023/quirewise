# Redact (Appendix A row 54)

## Purpose

Removes content from a PDF for good: text, pictures, lines and shapes, annotations and form values under areas you draw, and every place a word, pattern or kind of text is found. What is removed is taken out of the file, not only covered. The original file is never changed.

## Options

| Option | Type | Meaning |
|---|---|---|
| `areas` | list | Rectangles on pages: `{ "page": 2, "region": { "x", "y", "width", "height" } }`, as fractions of the displayed page from the top-left |
| `terms` | list | Words or phrases to find. A space in a phrase matches any run of spaces and line breaks. |
| `presets` | list | `email`, `phone`, `iban` (check digits must be right), `card-number` (13 to 19 digits that pass the Luhn check) |
| `patterns` | list | Regular expressions (Rust `regex` syntax, so they cannot take exponential time) |
| `case_sensitive` | bool | Match case of `terms` and `patterns`; default off |
| `pages` | text | Pages the search covers: `all`, `even`, `odd` or a range; default `all`. Drawn areas apply to their own page. |
| `fill_color` | text | Color of the boxes that cover what was removed; default `#000000` |
| `strip_metadata` | bool | Clear the document information and the XMP packet; default on |
| `ai_suggestions` | bool | Also remove the personal details a local language model finds: names, addresses, numbers that no pattern describes. Default off. |
| `ai` | group | The model for `ai_suggestions`; see `smart-split.md`, "The model" |

Something to remove is needed: an area, a word, a pattern, a preset or `ai_suggestions`.

```
qw run redact contract.pdf --options '{"presets":["email","card-number"],"terms":["Project Falcon"]}'
```

In the app the page editor shows the file. Draw areas with the Area tool; words, patterns and kinds of text are typed in the options, and the places they match are outlined on the pages as you type (nothing is changed until you run the tool). A search match covers only the characters that matched, not the whole word.

## AI suggestions

With `ai_suggestions` on, the model reads each searched page and lists personal details (names of people, postal addresses, telephone numbers, e-mail addresses, dates of birth, identity, tax, bank and card numbers). An item counts only when it appears in the page word for word, so nothing the model invents is searched for; the items are added to `terms` and found like any other word. The model can miss things and can name things that need not go: in the app, **Ask the model** adds the words to the list of words, where you read and change them, and then turns the option off, so the run does not ask again. Without the app (`qw run`), the run asks the model itself and removes what it finds without a review.

## How it works

1. **Finding.** The characters of each page come from PDFium with their boxes. The text is rebuilt (words and lines the content does not separate are separated, also for text that runs up or down the page), searched, and each match is turned back into the boxes of its own characters, one region for each line.
2. **Removing.** The content stream of each page is walked with the graphics and text state the PDF model defines, so the place of every character, picture and shape is known:
   - characters whose boxes meet an area are taken out of their strings; each is replaced by a gap of its advance, so the text that stays does not move. Simple fonts (with `Widths` or one of the standard fonts), Identity-encoded composite fonts and Type 3 fonts are followed exactly;
   - pictures that meet an area are decoded, the pixels inside are cleared and the picture is written again, losslessly (gray or RGB). The original picture is dropped from the file unless the page draws it somewhere else;
   - forms (reusable content) are cleaned on a copy, so pages that share one are not changed by another page's area;
   - lines and shapes that meet an area are not painted. A fill around an area (a table cell, a page background) stays, since one color lies under the area, and so does a frame whose sides are clear of the area;
   - annotations and form fields under an area are removed with their values, and so are the pop-ups of removed annotations;
   - the alternate text of marked content, thumbnails and the page's own metadata are dropped;
   - a box of the chosen color is drawn over every area.
3. **Checking.** The result is read back. Text found under an area, or a searched word that is still on a page, means that page is not trusted.
4. **Replacing.** A page the engine cannot understand well enough (pictures inside the content stream, fonts whose code maps are unknown on a line an area covers, stencil masks, scans stored as CCITT or JBIG2) or that fails the check is replaced by a picture of itself at 200 dpi with the boxes burned in. Such a page loses its text and selection but keeps its look.

A password-protected input is unlocked, redacted and protected again with the same settings.

## Behavior and limits

- Bookmark names, form field names, attachments, page labels and hidden layers are not searched or cleaned. Remove them with Sanitize, Edit metadata or the attachments tool when they may hold the same information.
- Alternate text kept in the document's structure tree (tagged PDFs) is not cleared; the marked content's own alternate text is.
- A shape that crosses an area is removed whole, not cut at the area's edge.
- Shadings and gradients are not examined; they carry no text.
- A page that is replaced by a picture cannot be searched or selected afterwards. Run OCR on the result when that matters.
- AI suggestions for what to redact (Appendix A row 54) arrive with the local AI component.
- The search reads the text PDFium reports. Text that exists only as a picture is not found; draw an area over it.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID` (nothing found to remove, a pattern that is not valid, an area outside the document), `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

`lopdf` (MIT), `regex` (MIT or Apache-2.0), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code is linked.
