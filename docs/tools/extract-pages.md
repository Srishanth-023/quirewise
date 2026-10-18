# Extract pages (Appendix A row 6)

## Purpose

Saves selected pages as a new PDF, or as one file per page.

## Options

| Option | Type | Meaning |
|---|---|---|
| `pages` | page range | default `all`. The order you type is the order in the result |
| `one_file` | bool | one output with all pages (default) or one file per page |

Command line (tool id `extract-pages`, options are JSON and missing fields use the defaults):

```
qw run extract-pages book.pdf --options '{"pages":"4,2,last"}'
```

## How it works

qpdf page copy. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- Pages may repeat; a repeated page appears twice in the output.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
