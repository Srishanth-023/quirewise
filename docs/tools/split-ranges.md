# Split by ranges (Appendix A row 4)

## Purpose

Cuts a PDF into files by page ranges. Each comma-separated element becomes one file.

## Options

| Option | Type | Meaning |
|---|---|---|
| `ranges` | page range | for example `1-3,4-6,7-`; default `all` |
| `merge_into_one` | bool | put all selected ranges into one file, default off |

Command line (tool id `split-ranges`, options are JSON and missing fields use the defaults):

```
qw run split-ranges book.pdf --options '{"ranges":"1-3,4-6,7-"}'
```

## How it works

The range parser resolves the elements; qpdf copies each group. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- Open ended ranges such as `7-` run to the last page. `last` and `last-2` are accepted.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
