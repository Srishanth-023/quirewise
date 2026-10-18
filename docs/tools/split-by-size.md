# Split by size (Appendix A row 10)

## Purpose

Cuts a PDF into files that stay under a size limit.

## Options

| Option | Type | Meaning |
|---|---|---|
| `max_size_mb` | int 1 to 2048 | largest output in MB (1 MB is 1,048,576 bytes); default 10 |

Command line (tool id `split-by-size`, options are JSON and missing fields use the defaults):

```
qw run split-by-size scan.pdf --options '{"max_size_mb":10}'
```

## How it works

Pages are packed greedily using measured page sizes (up to 300 pages, the average beyond that). Every output is written and measured; a file over the limit is split again until it fits. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- A single page larger than the limit becomes its own file and a warning says so.
- Estimates are conservative: files may be somewhat smaller than the limit (open-questions row 18).
- Shared resources such as fonts are counted in every file that needs them.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
