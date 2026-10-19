# Header and footer (Appendix A row 34)

## Purpose

Writes text in up to six places: left, center and right of the top and of the bottom edge.

## Options

| Option | Type | Meaning |
|---|---|---|
| `header_left, header_center, header_right` | text | tokens as in Page numbers; blank draws nothing |
| `footer_left, footer_center, footer_right` | text | same |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |
| `font, bold, italic, size_pt, color` | mixed | default Helvetica, 10 pt, black |
| `margin_pt` | number | distance from the page edge; default 36 |

```
qw run header-footer contract.pdf --options '{"header_center":"{filename}","footer_right":"{page} / {pages}"}'
```

## How it works

Uses the same stamper as Page numbers. At least one of the six texts is needed. Text uses the standard PDF fonts (Helvetica, Times, Courier; bold and italic), so it needs no embedding. Only Latin-1 and Windows-1252 characters can be drawn; anything else is rejected before the run.

## Behavior and limits

- The six texts share one font and size.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
