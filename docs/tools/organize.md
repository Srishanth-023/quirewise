# Organize (visual reorder) (Appendix A row 3)

## Purpose

Reorders, rotates, deletes, duplicates and inserts pages across one or more files, then writes the result in one pass.

## Options

| Option | Type | Meaning |
|---|---|---|
| `pages` | list | the result, in order. Each entry is `{"kind":"page","source":<input index>,"page":<page, both zero-based>,"rotate":<angle>}` or `{"kind":"blank","width_pt":<points>,"height_pt":<points>}`. Empty keeps the first file as it is |

Command line (tool id `organize`, options are JSON and missing fields use the defaults):

```
qw run organize a.pdf b.pdf --options '{"pages":[{"kind":"page","source":1,"page":0},{"kind":"blank","width_pt":612,"height_pt":792},{"kind":"page","source":0,"page":0,"rotate":"90"}]}'
```

## How it works

The UI edits a list of operations with undo and redo and sends the final page list. qpdf assembles it, with one generated blank PDF per distinct blank size. Outputs are written to a temporary name, verified (PDFium reopens it, `qpdf --check` passes, the page count matches), then published without overwriting anything. Inputs are never changed.

## Behavior and limits

- Every entry is checked against the real page counts before anything is written.
- Pages from different files carry a text source label in the editor, never color alone.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`.

## License impact

qpdf (Apache-2.0, run as a separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
