# Merge PDF (Appendix A row 1)

## Purpose

Combines PDFs and images into one PDF, in the order the files are listed.

## Options

| Option | Type | Meaning |
|---|---|---|
| `per_file` | list | one entry per input, in order: `pages` (default `all`) and `rotation` (`0`, `90`, `180`, `270`; default `0`) |
| `bookmark_per_file` | bool | add one bookmark per file, default on |
| `table_of_contents` | bool | insert a contents page listing the files, default off |
| `normalize_page_size` | bool | scale every page to the first page size, default off |
| `forms` | `keep` or `flatten` | keep fields fillable (default) or flatten them |

Command line (tool id `merge`, options are JSON and missing fields use the defaults):

```
qw run merge a.pdf b.pdf photo.png --options '{"per_file":[{"pages":"1-3"},{"rotation":"90"}]}'
```

## How it works

The domain plans the page list; the worker converts images to PDF pages (image crate, 150 dpi), decrypts locked sources with their password (passed on stdin, never on a command line), and qpdf assembles the result in one pass. Bookmarks and the contents page are written afterwards by lopdf. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- Images become one page each, sized at 150 dpi, so a small image gives a small page. Use `normalize_page_size` to unify sizes.
- The contents page lists file names and first pages, 35 lines per page. It has no clickable links yet (open-questions row 20).
- Scaling to the first page size keeps the aspect ratio and centers the page. Annotations are not moved (row 19).
- Locked files need their password; each file keeps its own.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code. image (MIT or Apache-2.0), lopdf (MIT).
