# Bookmarks (Appendix A row 49)

## Purpose

Sets the bookmark list: type the entries, or build them from the large text of the document.

## Options

| Option | Type | Meaning |
|---|---|---|
| `bookmarks` | rows | title, page (from 1) and level (1 is the top); the list replaces the current bookmarks |
| `auto_generate` | switch | build the list from headings instead; default off |
| `auto_max_level` | number | deepest generated level, 1 to 6; default 3 |

```
qw run bookmarks report.pdf --options '{"bookmarks":[{"title":"Intro","page":1,"level":1},{"title":"Scope","page":2,"level":2}]}'
```

## How it works

Entries are checked (a level may be at most one deeper than the one before, pages must exist) and written as a new outline. Headings are found by grouping the characters of each page into lines and ranking the font sizes larger than the body text; the largest sizes become levels 1 to `auto_max_level`.

## Behavior and limits

- Headings are a guess: scanned files without a text layer have none. Review the result.
- An empty list is rejected; there is nothing to write.
- The form starts empty and the list replaces the bookmarks the file already has, so retype the ones you want to keep.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process), PDFium for heading detection (BSD-3-Clause and Apache-2.0). No copyleft code.
