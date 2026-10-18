# Split in half (Appendix A row 9)

## Purpose

Cuts every page into two pages, for example scans of two-page spreads.

## Options

| Option | Type | Meaning |
|---|---|---|
| `direction` | `vertical` or `horizontal` | vertical cuts into left and right, horizontal into top and bottom; default vertical |
| `skip_first` | bool | leave the first page whole, default off |
| `skip_last` | bool | leave the last page whole, default off |
| `right_to_left` | bool | write the right half first, default off |

Command line (tool id `split-in-half`, options are JSON and missing fields use the defaults):

```
qw run split-in-half spreads.pdf --options '{"direction":"vertical","right_to_left":true}'
```

## How it works

Each page is duplicated by qpdf, then lopdf sets the CropBox of the copies to the two halves. No content is rasterized or lost. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- The cut follows the displayed page, so rotated pages are halved as they look.
- The full page content stays in the file under the crop; use Optimize later to drop it.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code. lopdf (MIT).
