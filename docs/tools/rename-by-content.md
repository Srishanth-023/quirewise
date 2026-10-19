# Rename by content (Appendix A row 50)

## Purpose

Saves a copy of each file under a name made from text read in a box on one of its pages.

## Options

| Option | Type | Meaning |
|---|---|---|
| `region` | box | drawn on the first page in the UI; required |
| `page` | number | page to read, counting from 1; default 1 |
| `pattern` | text | new name with `{text}` for the text read, plus `{filename}`, `{date}` and the other naming tokens; default `{text}` |

```
qw run rename-by-content invoices/*.pdf --options '{"region":{"x":0.6,"y":0.05,"width":0.3,"height":0.05},"pattern":"Invoice {text}"}'
```

## How it works

PDFium reads the text inside the box of the displayed page. The text is reduced to one line, made safe as a file name and put in the pattern. The input bytes are copied unchanged, so the content, signatures and protection stay exactly as they were.

## Behavior and limits

- A box with no text (for example on a scan) fails that file with a message. Run OCR first.
- A name that is already in the output folder gets a number; nothing is overwritten.
- The original is never renamed or removed: the renamed copy is written to the output folder.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
