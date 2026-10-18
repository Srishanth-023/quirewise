# Split by bookmarks (Appendix A row 8)

## Purpose

Cuts a PDF at the bookmarks of one outline level, one file per bookmark.

## Options

| Option | Type | Meaning |
|---|---|---|
| `level` | int, at least 1 | outline level, 1 is the top level; default 1 |
| `name_from_bookmark` | bool | name each file after its bookmark title, default on |

Command line (tool id `split-by-bookmarks`, options are JSON and missing fields use the defaults):

```
qw run split-by-bookmarks report.pdf --options '{"level":1}'
```

## How it works

PDFium reads the outline (levels come from the parent chain); the domain groups pages from each bookmark to the next; qpdf copies them. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- Pages before the first bookmark become a leading file named `front matter`. Bookmarks that point at the same page start one file.
- A file with no bookmarks at that level gives `QW-OPTION-INVALID` ("this file has no bookmarks at level N").
- Bookmark titles are made safe for Windows file names (illegal characters, reserved names, length).

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
