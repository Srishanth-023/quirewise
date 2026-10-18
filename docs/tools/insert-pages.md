# Insert blank or duplicate pages (Appendix A row 69)

## Purpose

Adds blank pages, or copies of pages, before or after selected pages.

## Options

| Option | Type | Meaning |
|---|---|---|
| `mode` | `blank` or `duplicate` | default blank |
| `pages` | page range | pages to insert around, default `1` |
| `position` | `before` or `after` | default after |
| `count` | int 1 to 100 | pages inserted at each place, default 1 |
| `page_size` | `same-as-neighbor`, `a4`, `letter`, `legal`, `a3` | size of blank pages, default same as the neighboring page |

Command line (tool id `insert-pages`, options are JSON and missing fields use the defaults):

```
qw run insert-pages book.pdf --options '{"mode":"blank","pages":"1,5","position":"after","count":2,"page_size":"a4"}'
```

## How it works

The domain insert planner builds the page list; qpdf assembles it, with one generated blank PDF per distinct size. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- Duplicates share the original page content, so the file grows only by the page objects.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
