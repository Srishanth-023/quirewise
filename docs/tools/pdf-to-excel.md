# PDF to Excel (Appendix A row 27)

## Purpose

Finds the tables of a PDF and writes them to a workbook (`.xlsx`) or to CSV files.

## Options

| Option | Type | Meaning |
|---|---|---|
| `format` | choice | `xlsx` (default) or `csv` |
| `grouping` | choice | `per-table` (one sheet for each table, default) or `per-page` (the tables of a page one under the other) |
| `detection` | choice | `auto` (lines first, then aligned text, default), `lattice` (lines only) or `stream` (aligned text only) |
| `ocr` | choice | `auto` (read pages that are only a picture), `off` or `always`; default `auto` |
| `languages` | list | Tesseract codes for OCR, one to six; default `eng` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |

```
qw run pdf-to-excel statement.pdf --options '{"format":"csv","detection":"lattice"}'
```

## How it works

A ruled table is found from its lines: thin bars and the sides of rectangles are joined into grids, the grid lines give the rows and columns and the words fall into the cells they sit in. A table without lines is found from text that lines up: a stretch of at least three lines with the same gaps, at least two columns, most cells filled and short cells (long sentences in two columns are text, not a table). Sheets are named "Page 3" or "Page 3 Table 2". In a workbook a cell whose text is a plain number (digits, a minus sign, thousands commas, decimals; no leading zeros) is a number, anything else is text. CSV files follow RFC 4180 with CR LF line ends and a byte order mark so Excel reads the characters correctly; one file is written for each sheet. PDFium gives the words (with size, bold and italic), the lines and bars that may be table borders and the pictures of every page, in points on the page as it is displayed, so rotated pages come out upright. A page that has no text but is covered by a picture is rendered at 300 dpi and read by Tesseract, which gives the words and their boxes; the picture of the scan is then left out. A picture that nearly fills a page that has real text is the scan the text was read from and is left out too. The words of all pages are then analyzed by `domain::layout` (pure code, tested on its own): words are grouped into lines and lines into segments, running headers, footers and page numbers that repeat on most pages are removed, tables are found (ruled tables from the lines, others from text that lines up in columns), columns are put in reading order by cutting the page at wide gutters, lines run into paragraphs, larger or short bold lines become headings (levels by size), and bullets and numbers become list items.

## Behavior and limits

- Merged cells are not detected: a cell that spans columns is written in its first column and the others are empty.
- Currency signs, percent signs and dates stay text, so Excel does not guess a format for them.
- When no table is found the run stops with a message and writes nothing.
- The analysis follows rules, not a trained model. Unusual layouts (text around pictures, magazine pages, forms) may need fixing by hand in the result.
- A scanned page needs the Tesseract component and the language data (Settings, Components). With text recognition off, or when a page has no picture, such a page stays a picture or empty.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`. A scanned page that needs text recognition when it is not installed is `QW-COMPONENT-MISSING`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0), the `zip` crate (MIT); Tesseract (Apache-2.0) as a component for scans. No copyleft code is linked.
