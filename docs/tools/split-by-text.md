# Split by text (Appendix A row 11)

## Purpose

Starts a new file whenever the text in a chosen region changes, for example an invoice number in a header.

## Options

| Option | Type | Meaning |
|---|---|---|
| `region` | object | required. Fractions of the page measured from the top left: `x`, `y`, `width`, `height`, each 0 to 1. The UI draws it on a page preview |
| `starts_with` | text | start a new file only on pages whose text begins with this (case ignored), default none |

Command line (tool id `split-by-text`, options are JSON and missing fields use the defaults):

```
qw run split-by-text invoices.pdf --options '{"region":{"x":0.55,"y":0.04,"width":0.4,"height":0.06},"starts_with":"INV"}'
```

## How it works

PDFium reads the text inside the region on every page (the unrotated page box is mapped to the region, so rotated pages work); the domain starts a group when the text changes (and, with a filter, begins with it); qpdf copies the groups. The `{text}` naming token receives the region text. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- A page with no text in the region continues the previous file.
- Scanned pages have no text layer; run OCR first.
- Region text is trimmed and made safe for file names.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
