# PDF to HTML (Appendix A row 67)

## Purpose

Writes a PDF as a web page with headings, paragraphs, lists, tables and pictures.

## Options

| Option | Type | Meaning |
|---|---|---|
| `inline_images` | switch | put the pictures inside the page, so it is one file; default on |
| `ocr` | choice | `auto` (read pages that are only a picture), `off` or `always`; default `auto` |
| `languages` | list | Tesseract codes for OCR, one to six; default `eng` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |

```
qw run pdf-to-html brochure.pdf --options '{"inline_images":false}'
```

## How it works

PDFium gives the words (with size, bold and italic), the lines and bars that may be table borders and the pictures of every page, in points on the page as it is displayed, so rotated pages come out upright. A page that has no text but is covered by a picture is rendered at 300 dpi and read by Tesseract, which gives the words and their boxes; the picture of the scan is then left out. A picture that nearly fills a page that has real text is the scan the text was read from and is left out too. The words of all pages are then analyzed by `domain::layout` (pure code, tested on its own): words are grouped into lines and lines into segments, running headers, footers and page numbers that repeat on most pages are removed, tables are found (ruled tables from the lines, others from text that lines up in columns), columns are put in reading order by cutting the page at wide gutters, lines run into paragraphs, larger or short bold lines become headings (levels by size), and bullets and numbers become list items. The page is semantic HTML (`h1` to `h3`, `p`, nested `ul` and `ol`, `table` with a header row, `img` with a text for screen readers) with a small built-in style that is readable on a phone, and no scripts. With inline pictures the page holds them as base64 data; otherwise they are saved beside it and linked. All text is escaped.

## Behavior and limits

- The analysis follows rules, not a trained model. Unusual layouts (text around pictures, magazine pages, forms) may need fixing by hand in the result.
- Fonts are not kept: text is set in Calibri with the size, bold and italic it had. Text colors, shading and vector drawings (charts, logos drawn as paths) are not carried over; pictures are.
- Pictures that sit inside a form object are not seen (see docs/project/open-questions.md). Pictures are limited to 2400 pixels on the long side; opaque pictures of 160000 pixels or more are stored as JPEG, the rest as PNG.
- Text that is not horizontal on the page (vertical lines, text turned by an angle) is read one character at a time and comes out in pieces.
- A scanned page needs the Tesseract component and the language data (Settings, Components). With text recognition off, or when a page has no picture, such a page stays a picture or empty.
- The page is not a copy of the printed layout: it reflows to the window. For a look-alike use PDF to images or PDF to Word with exact positions.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`. A scanned page that needs text recognition when it is not installed is `QW-COMPONENT-MISSING`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0); Tesseract (Apache-2.0) as a component for scans. No copyleft code is linked.
