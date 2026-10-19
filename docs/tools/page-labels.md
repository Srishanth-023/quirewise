# Page labels (Appendix A row 66)

## Purpose

Sets how pages are numbered in a viewer, for example i, ii, iii for the front matter and 1, 2, 3 after it.

## Options

| Option | Type | Meaning |
|---|---|---|
| `ranges` | rows | each run has a first page (from 1), a style (`arabic`, `roman-lower`, `roman-upper`, `letters-lower`, `letters-upper`), a prefix and a start number |
| `remove_all` | switch | remove all page labels instead; default off |

```
qw run page-labels book.pdf --options '{"ranges":[{"from_page":1,"style":"roman-lower"},{"from_page":5,"style":"arabic","start":1}]}'
```

## How it works

Writes a `/PageLabels` number tree. Runs are sorted by first page, two runs cannot start on the same page, and pages before the first run are labeled 1, 2, 3.

## Behavior and limits

- Labels change only what a viewer shows; the physical page order is unchanged.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
