# Extract images (Appendix A row 24)

## Purpose

Takes the pictures out of a PDF as files.

## Options

| Option | Type | Meaning |
|---|---|---|
| `keep_original_format` | switch | keep the format of each picture; off saves PNG; default on |
| `min_width, min_height` | number | skip pictures smaller than this many pixels; default 50 by 50 |

```
qw run extract-images brochure.pdf --options '{"min_width":100,"min_height":100}'
```

## How it works

JPEG and JPEG 2000 pictures are written exactly as the PDF stores them, so a picture put in by Images to PDF comes out byte for byte. Raw and Flate pictures in gray, RGB, CMYK, ICC based and indexed color spaces become PNG, with their soft mask as transparency and stencil masks as black on white. A picture used on several pages is written once, at the first page that shows it. Files are named `report (page 2 image 1).jpg`.

## Behavior and limits

- Scans stored as CCITT or JBIG2 cannot be taken out as files; use PDF to images for those pages. The message says how many were left out.
- An encrypted PDF is opened with its password first (a decrypted copy is made in the scratch folder and removed).
- The tool reads PDF picture objects with lopdf, not PDFium (see docs/open-questions.md).

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), the `image` crate (MIT or Apache-2.0). No copyleft code is linked.
