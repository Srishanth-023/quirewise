# Flatten (Appendix A row 47)

## Purpose

Turns form fields and annotations into fixed page content, or turns every page into a picture.

## Options

| Option | Type | Meaning |
|---|---|---|
| `mode` | `forms-and-annotations`, `everything` | default `forms-and-annotations` |
| `dpi` | `75`, `100`, `150`, `220` | resolution when `mode` is `everything`; default 150 |

```
qw run flatten form.pdf --options '{"mode":"everything","dpi":"150"}'
```

## How it works

`forms-and-annotations`: PDFium flattens each page, which bakes the appearance of form fields and annotations into the page content. Text and vectors stay as they were.

`everything`: every page is rendered to a PNG at the chosen dpi and the result is an image-only PDF with the original page sizes. Output is verified and published without overwriting.

## Behavior and limits

- In `everything` mode text can no longer be selected or searched, links and bookmarks are lost, and the file can be much larger.
- Signatures become invalid because the file changes.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`).
- Fields without an appearance stream have nothing to draw.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0), the `image` crate (MIT or Apache-2.0), qpdf (Apache-2.0, separate process). No copyleft code.
