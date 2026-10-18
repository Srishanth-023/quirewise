# Alternate and Mix (Appendix A row 2)

## Purpose

Interleaves the pages of two or more documents, for example the front and back sides of a duplex scan.

## Options

| Option | Type | Meaning |
|---|---|---|
| `pages_per_turn` | int 1 to 1000 | pages taken from each file per turn, default 1 |
| `per_file` | list | one entry per input: `reverse` reads that file from its last page backwards, default off |

Command line (tool id `alternate-mix`, options are JSON and missing fields use the defaults):

```
qw run alternate-mix fronts.pdf backs.pdf --options '{"per_file":[{},{"reverse":true}]}'
```

## How it works

The domain interleave planner builds the page order; qpdf copies the pages. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- Needs at least two inputs. When one file runs out, the others continue in order.
- Reversing the back-side file is how two passes of an automatic feeder scan are zipped into reading order.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
