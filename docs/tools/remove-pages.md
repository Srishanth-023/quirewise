# Remove pages (Appendix A row 7)

## Purpose

Saves a copy of the PDF without the selected pages.

## Options

| Option | Type | Meaning |
|---|---|---|
| `pages` | page range | pages to remove, default `1` |

Command line (tool id `remove-pages`, options are JSON and missing fields use the defaults):

```
qw run remove-pages book.pdf --options '{"pages":"2,4-6"}'
```

## How it works

The domain computes the pages that stay; qpdf copies them. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- Removing every page is refused: `QW-OPTION-INVALID`, because nothing would be left.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
