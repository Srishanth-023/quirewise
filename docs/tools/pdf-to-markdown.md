# PDF to Markdown (Appendix A row 29)

## Purpose

Writes a PDF as a Markdown file, with its structure, for notes, wikis and tools that read Markdown.

## Options

| Option | Type | Meaning |
|---|---|---|
| `images` | switch | save the pictures next to the file and link them; default on |
| `ocr` | choice | `auto` (read pages that are only a picture), `off` or `always`; default `auto` |
| `languages` | list | Tesseract codes for OCR, one to six; default `eng` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |

```
qw run pdf-to-markdown paper.pdf --options '{"images":false}'
```

## How it works

PDFium gives the words (with size, bold and italic), the lines and bars that may be table borders and the pictures of every page, in points on the page as it is displayed, so rotated pages come out upright. A page that has no text but is covered by a picture is rendered at 300 dpi and read by Tesseract, which gives the words and their boxes; the picture of the scan is then left out. A picture that nearly fills a page that has real text is the scan the text was read from and is left out too. The words of all pages are then analyzed by `domain::layout` (pure code, tested on its own): words are grouped into lines and lines into segments, running headers, footers and page numbers that repeat on most pages are removed, tables are found (ruled tables from the lines, others from text that lines up in columns), columns are put in reading order by cutting the page at wide gutters, lines run into paragraphs, larger or short bold lines become headings (levels by size), and bullets and numbers become list items. Headings become `#` to `###`, lists become `-` and `1.` items with two spaces of indent for each level, tables become pipe tables (the first row is the header), bold and italic become `**` and `*`. Characters Markdown would read as markup are escaped with a backslash. Pictures are saved as PNG or JPEG files in the output folder, named like any output, and linked with a relative path in which spaces and brackets are percent-encoded. The file is UTF-8 with LF line ends.

## Behavior and limits

- The analysis follows rules, not a trained model. Unusual layouts (text around pictures, magazine pages, forms) may need fixing by hand in the result.
- Fonts are not kept: text is set in Calibri with the size, bold and italic it had. Text colors, shading and vector drawings (charts, logos drawn as paths) are not carried over; pictures are.
- Pictures that sit inside a form object are not seen (see docs/project/open-questions.md). Pictures are limited to 2400 pixels on the long side; opaque pictures of 160000 pixels or more are stored as JPEG, the rest as PNG.
- Text that is not horizontal on the page (vertical lines, text turned by an angle) is read one character at a time and comes out in pieces.
- A scanned page needs the Tesseract component and the language data (Settings, Components). With text recognition off, or when a page has no picture, such a page stays a picture or empty.
- Markdown has no page breaks or columns: the pages follow one another and columns are read one after the other.
- The AI clean-up (`ai_cleanup`, off by default) sends the prose of the result to the local model to join lines the page width broke, mend words broken by a hyphen and fix heading levels. Tables, pictures and code are never sent. A tidied piece is kept only when it has the same words as the original (a twelfth of them, at least three, may differ, for the words that joining and mending change); otherwise the original piece stays and the log records that. Words are never added, removed or translated.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`. A scanned page that needs text recognition when it is not installed is `QW-COMPONENT-MISSING`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0); Tesseract (Apache-2.0) as a component for scans. No copyleft code is linked.
