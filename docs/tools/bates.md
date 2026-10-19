# Bates numbering (Appendix A row 35)

## Purpose

Gives every page a running number with a prefix and a suffix, continuing from one file to the next.

## Options

| Option | Type | Meaning |
|---|---|---|
| `prefix, suffix` | text | for example `ACME-`; default none |
| `digits` | number | zero padding, 1 to 12; default 6 |
| `start` | number | first number; default 1 |
| `continue_across_files` | switch | keep counting in the order of the file list; default on |
| `position, offset_x_pt, offset_y_pt` | mixed | default `bottom-right` |
| `font, bold, italic, size_pt, color` | mixed | default Helvetica, 10 pt, black |

```
qw run bates a.pdf b.pdf --options '{"prefix":"ACME-","digits":6}'
```

## How it works

All files run together, in list order, so the counter can continue. Each input produces its own output file; with `continue_across_files` off every file restarts at `start`. Text uses the standard PDF fonts (Helvetica, Times, Courier; bold and italic), so it needs no embedding. Only Latin-1 and Windows-1252 characters can be drawn; anything else is rejected before the run.

## Behavior and limits

- Reordering the file list changes the numbers: they follow the list as it is when the run starts.
- The number is placed by the Page numbers rules (36 pt from the edge, upright on rotated pages).
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
