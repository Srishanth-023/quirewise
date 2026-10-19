# Pages per sheet (N-up) (Appendix A row 40)

## Purpose

Puts 2, 4, 8, 16 or 32 pages on each sheet.

## Options

| Option | Type | Meaning |
|---|---|---|
| `per_sheet` | choice | `2`, `4`, `8`, `16` or `32`; default 2 |
| `order` | choice | `horizontal` (across, then down) or `vertical`; default `horizontal` |
| `preserve_page_size` | switch | sheets keep the size of the first page and the pages shrink; off makes the sheet as large as the grid of full-size pages; default on |
| `right_to_left` | switch | fill each row from the right; default off |

```
qw run n-up handout.pdf --options '{"per_sheet":"4"}'
```

## How it works

Each source page becomes a form XObject placed in a grid cell with its own matrix. Rotated pages are placed upright. The last sheet may have empty cells.

## Behavior and limits

- Links, form fields, bookmarks and page labels of the source are not carried over to the sheets.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
