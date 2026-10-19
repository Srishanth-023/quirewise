# Images to PDF (Appendix A row 18)

## Purpose

Puts pictures on the pages of a PDF: one page per picture, in one file or in one file each.

## Options

| Option | Type | Meaning |
|---|---|---|
| `one_pdf` | switch | all pictures in one PDF; off makes one PDF per picture; default on |
| `page_size` | choice | `fit` (the page is as large as the picture at 150 dpi), `a4` or `letter`; default `fit` |
| `orientation` | choice | `auto` (follows the picture), `portrait` or `landscape`, for A4 and Letter; default `auto` |
| `margin` | choice | `none`, `small` (10 mm) or `big` (25 mm); default `none` |

```
qw run images-to-pdf a.jpg b.png --options '{"page_size":"a4","margin":"small"}'
```

## How it works

JPEG pictures are embedded exactly as they are, with no new compression. Other formats are decoded and stored without loss, with transparency kept as a soft mask. A JPEG that asks for a turn in its EXIF block (phone photos) is shown upright by a matrix on the page; its bytes stay as they were. On A4 and Letter the picture is scaled to the largest size that fits inside the margins and centered. HEIC pictures are decoded by Windows through PowerShell, which needs the HEIF Image Extensions from the Microsoft Store.

## Behavior and limits

- The result is checked (reopened, `qpdf --check`, page count) before it is saved, and the pictures are never changed.
- Pictures larger than 100 megapixels are refused.
- Multi-page TIFF files are read by TIFF to PDF; here only the first frame is used.
- Animated GIFs use their first frame.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), the `image` crate (MIT or Apache-2.0), Windows PowerShell for HEIC. No copyleft code is linked.
