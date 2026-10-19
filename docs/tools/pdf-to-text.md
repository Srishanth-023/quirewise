# PDF to text (Appendix A row 28)

## Purpose

Writes the text of a PDF to a text file.

## Options

| Option | Type | Meaning |
|---|---|---|
| `layout` | choice | `plain` (reading order) or `layout` (keeps columns and gaps); default `plain` |
| `page_separators` | switch | a line such as `--- Page 2 ---` between pages; default on |
| `encoding` | choice | `utf8`, `utf8-bom`, `utf16` or `windows1252`; default `utf8` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |

```
qw run pdf-to-text invoice.pdf --options '{"layout":"layout","encoding":"utf8-bom"}'
```

## How it works

PDFium reads the text. In layout mode every word is placed in the character column its position on the page gives, using the width of an average letter as the column, and gaps between lines become blank lines. Line breaks are written as Windows line breaks.

## Behavior and limits

- A scan has no text, so the file has only the page lines. Run OCR first.
- Windows-1252 turns characters it lacks into `?`.
- Layout mode does not follow rotated text.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0). No copyleft code is linked.
