# Scan to PDF (Appendix A row 13)

## Purpose

Turns pictures of pages, from a scanner or a camera, into a PDF.

## Options

| Option | Type | Meaning |
|---|---|---|
| `auto_crop` | switch | cut away what is not paper; default on |
| `perspective_fix` | switch | stand pages photographed at an angle upright; default on |
| `color` | choice | `color`, `gray` or `black-white`; default `color` |
| `dpi` | number | resolution of the scan, which sets the page size, 100 to 600; default 300 |
| `ocr` | switch | make the PDF searchable; default off |
| `languages` | list | for OCR; default `eng` |

```
qw run scan-to-pdf page1.jpg page2.jpg --options '{"color":"black-white","ocr":true}'
```

## How it works

The pictures in the list are the pages, in list order. The paper is found as the largest bright area; its four corners are the points farthest along the diagonals. A straight sheet is only cropped; a tilted or photographed one is warped to a rectangle with a projective transform. Black and white uses a local threshold, so a photo with uneven light keeps its text, and is stored as CCITT Group 4; gray and color pages are JPEG. The Scan section of the home screen gets the pictures: the Windows scanning dialog (Windows Image Acquisition, through PowerShell) or the camera preview. They are saved in Documents, Quirewise Scans.

## Behavior and limits

- The scanner and camera paths use hardware and the Windows dialog; they could not be tried without a device in the automated tests (see docs/open-questions.md).
- Paper that is not brighter than its background is not found; turn the two clean-up options off for those pictures.
- With OCR on, the pages are read like any scanned PDF and the result is searchable.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`. A missing component is `QW-COMPONENT-MISSING`.

## License impact

The `image` crate (MIT or Apache-2.0), `fax` (MIT), lopdf (MIT); Tesseract as a component for OCR. No copyleft code is linked.
