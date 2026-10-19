# PDF to Word (Appendix A row 25)

## Purpose

Turns a PDF into a Word document (`.docx`) with headings, paragraphs, lists, tables and pictures you can edit.

## Options

| Option | Type | Meaning |
|---|---|---|
| `layout` | choice | `flowing` (text that reflows, default) or `exact` (every block placed where it was) |
| `ocr` | choice | `auto` (read pages that are only a picture), `off` or `always`; default `auto` |
| `languages` | list | Tesseract codes for OCR, one to six; default `eng` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |

```
qw run pdf-to-word report.pdf --options '{"layout":"flowing","pages":"1-3"}'
```

## How it works

PDFium gives the words (with size, bold and italic), the lines and bars that may be table borders and the pictures of every page, in points on the page as it is displayed, so rotated pages come out upright. A page that has no text but is covered by a picture is rendered at 300 dpi and read by Tesseract, which gives the words and their boxes; the picture of the scan is then left out. A picture that nearly fills a page that has real text is the scan the text was read from and is left out too. The words of all pages are then analyzed by `domain::layout` (pure code, tested on its own): words are grouped into lines and lines into segments, running headers, footers and page numbers that repeat on most pages are removed, tables are found (ruled tables from the lines, others from text that lines up in columns), columns are put in reading order by cutting the page at wide gutters, lines run into paragraphs, larger or short bold lines become headings (levels by size), and bullets and numbers become list items. Flowing mode writes Heading 1 to 3, normal paragraphs, bulleted and numbered lists (a numbered list starts at the number it had), tables with a grid and inline pictures, on a page of the size of the first PDF page with one inch margins. Exact mode writes one section per page; every paragraph is in a frame at its position, tables are floating tables and pictures are anchored to the page. The file is a hand-written Open XML package, so Word is not needed to make it.

## Behavior and limits

- The analysis follows rules, not a trained model. Unusual layouts (text around pictures, magazine pages, forms) may need fixing by hand in the result.
- Fonts are not kept: text is set in Calibri with the size, bold and italic it had. Text colors, shading and vector drawings (charts, logos drawn as paths) are not carried over; pictures are.
- Pictures that sit inside a form object are not seen (see docs/open-questions.md). Pictures are limited to 2400 pixels on the long side; opaque pictures of 160000 pixels or more are stored as JPEG, the rest as PNG.
- Text that is not horizontal on the page (vertical lines, text turned by an angle) is read one character at a time and comes out in pieces.
- A scanned page needs the Tesseract component and the language data (Settings, Components). With text recognition off, or when a page has no picture, such a page stays a picture or empty.
- Exact mode looks like the page but is harder to edit; because the fonts differ, a line can wrap earlier or later than in the PDF.
- The result is checked by reopening the package and looking for its main part. In the automated tests Microsoft Word opens the files again and the text and positions are compared.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`. A scanned page that needs text recognition when it is not installed is `QW-COMPONENT-MISSING`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0), the `zip` crate (MIT), the `image` crate (MIT or Apache-2.0); Tesseract (Apache-2.0) as a component for scans. No copyleft code is linked.
