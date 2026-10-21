# PDF to PowerPoint (Appendix A row 26)

## Purpose

Turns each page of a PDF into a slide, as editable text boxes, tables and pictures or as one picture.

## Options

| Option | Type | Meaning |
|---|---|---|
| `mode` | choice | `editable-text` (default) or `image-slides` (one picture per slide) |
| `dpi` | number | resolution of the pictures for image slides, 100 to 600; default 150 |
| `ocr` | choice | `auto` (read pages that are only a picture), `off` or `always`; default `auto` |
| `languages` | list | Tesseract codes for OCR, one to six; default `eng` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |

```
qw run pdf-to-powerpoint deck.pdf --options '{"mode":"image-slides","dpi":200}'
```

## How it works

Editable slides use the analysis below. Every block gets a text box at its position (a little wider, so text does not wrap sooner), list items get bullets or automatic numbers, tables become PowerPoint tables with borders, and pictures keep their place. The slide has the size of the first page; pages of another size are scaled to fit. Image slides render each page at the chosen resolution and put it, as a JPEG, on a slide of the page size. PDFium gives the words (with size, bold and italic), the lines and bars that may be table borders and the pictures of every page, in points on the page as it is displayed, so rotated pages come out upright. A page that has no text but is covered by a picture is rendered at 300 dpi and read by Tesseract, which gives the words and their boxes; the picture of the scan is then left out. A picture that nearly fills a page that has real text is the scan the text was read from and is left out too. The words of all pages are then analyzed by `domain::layout` (pure code, tested on its own): words are grouped into lines and lines into segments, running headers, footers and page numbers that repeat on most pages are removed, tables are found (ruled tables from the lines, others from text that lines up in columns), columns are put in reading order by cutting the page at wide gutters, lines run into paragraphs, larger or short bold lines become headings (levels by size), and bullets and numbers become list items.

## Behavior and limits

- The analysis follows rules, not a trained model. Unusual layouts (text around pictures, magazine pages, forms) may need fixing by hand in the result.
- Fonts are not kept: text is set in Calibri with the size, bold and italic it had. Text colors, shading and vector drawings (charts, logos drawn as paths) are not carried over; pictures are.
- Pictures that sit inside a form object are not seen (see docs/project/open-questions.md). Pictures are limited to 2400 pixels on the long side; opaque pictures of 160000 pixels or more are stored as JPEG, the rest as PNG.
- Text that is not horizontal on the page (vertical lines, text turned by an angle) is read one character at a time and comes out in pieces.
- A scanned page needs the Tesseract component and the language data (Settings, Components). With text recognition off, or when a page has no picture, such a page stays a picture or empty.
- The slides use a blank layout; no title or content placeholders are created, so the Outline view of PowerPoint stays empty.
- Image slides cannot be edited and make a larger file; use them when the look matters more than the text.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`. A scanned page that needs text recognition when it is not installed is `QW-COMPONENT-MISSING`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0), the `zip` crate (MIT), the `image` crate (MIT or Apache-2.0); Tesseract (Apache-2.0) as a component for scans. No copyleft code is linked.
