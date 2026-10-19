# PDF to images (Appendix A row 23)

## Purpose

Saves pages as JPG, PNG or TIFF pictures.

## Options

| Option | Type | Meaning |
|---|---|---|
| `format` | choice | `jpg`, `png` or `tiff`; default `jpg` |
| `dpi` | number | 10 to 2400; default 150 |
| `jpeg_quality` | number | 1 to 100, for JPG; default 90 |
| `color` | choice | `color`, `gray` or `black-white`; default `color` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |
| `single_tiff` | switch | for TIFF: one multi-page file; default off |

```
qw run pdf-to-images report.pdf --options '{"format":"png","dpi":220,"pages":"1-3"}'
```

## How it works

PDFium renders each page. Every picture is named from the output pattern with the page number (`report (page 3).png`). Black and white uses the Otsu threshold, so scans with uneven lighting keep their text. Pages are rendered one at a time, so a long document does not fill memory. The single TIFF is written page by page (LZW).

## Behavior and limits

- JPG and TIFF files record the resolution; PNG files do not.
- Transparent areas come out white.
- Each written picture is decoded again before it is saved.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0), the `image` crate (MIT or Apache-2.0). No copyleft code is linked.
