# Rotate (Appendix A row 32)

## Purpose

Rotates pages by 90, 180 or 270 degrees clockwise.

## Options

| Option | Type | Meaning |
|---|---|---|
| `angle` | `0`, `90`, `180`, `270` | clockwise, default 90 |
| `pages` | page range | `all`, `even`, `odd` or a range such as `1,3-5`; default `all` |

Command line (tool id `rotate`, options are JSON and missing fields use the defaults):

```
qw run rotate scan.pdf --options '{"angle":"90","pages":"even"}'
```

## How it works

qpdf `--rotate=+angle:pages` changes the page `/Rotate` value. Content is not rewritten. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- The rotation adds to any rotation the page already has.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
