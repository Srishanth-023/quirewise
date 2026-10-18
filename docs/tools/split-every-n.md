# Split every N pages (Appendix A row 5)

## Purpose

Cuts a PDF into files of N pages. The last file holds the remainder.

## Options

| Option | Type | Meaning |
|---|---|---|
| `n` | int, at least 1 | pages per file, default 1 |

Command line (tool id `split-every-n`, options are JSON and missing fields use the defaults):

```
qw run split-every-n book.pdf --options '{"n":10}'
```

## How it works

The domain `every_n` planner groups the pages; qpdf copies each group. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- An N of zero is rejected before a worker starts.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
