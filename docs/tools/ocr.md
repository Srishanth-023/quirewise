# OCR (Appendix A row 16)

## Purpose

Reads scanned pages and makes the PDF searchable, or writes the recognized text to a file.

## Options

| Option | Type | Meaning |
|---|---|---|
| `languages` | list | Tesseract codes such as `eng`, `deu`, `chi_sim`; one to six; default `eng`. The form offers the installed ones |
| `output` | choice | `pdf` (the document with an invisible text layer) or `text`; default `pdf` |
| `deskew` | switch | straighten tilted pages first; default off |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |
| `skip_pages_with_text` | switch | leave pages that already have text alone; default on |
| `dpi` | number | resolution pages are read at, 100 to 600; default 300 |

```
qw run ocr scan.pdf --options '{"languages":["eng","deu"],"deskew":true}'
```

## How it works

Each page is rendered by PDFium and read by Tesseract. For a PDF, Tesseract writes a one-page PDF that holds only invisible text ("textonly"), sized from the picture; that page is drawn over the real page as a Form XObject scaled to the page as displayed, so the scan itself, its quality and everything else on the page are untouched. A page counts as having text when it has ten or more letters or digits. Deskew measures each page, turns the page and the picture that is read by the same angle about the same center, so the text lands on the words. A protected input stays protected.

## Behavior and limits

- Needs the Tesseract component and the language data (Settings, Components). A missing language is named before any work starts.
- When every chosen page already has text and the output is a PDF, the run stops with a message instead of saving a copy.
- Text output uses the page's own text for pages that have it, and reads the others.
- Accuracy depends on the scan: 300 dpi and straight pages read best. Handwriting is not recognized.
- Right-to-left and vertical scripts are placed by Tesseract; check the result before relying on it.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`. A missing component is `QW-COMPONENT-MISSING`.

## License impact

Tesseract (Apache-2.0) and Leptonica (BSD-2-Clause) as components, language data (Apache-2.0), PDFium (BSD-3-Clause and Apache-2.0), lopdf (MIT). No copyleft code is linked.
