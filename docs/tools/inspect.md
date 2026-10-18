# Inspect and Properties (Appendix A row 71)

## Purpose

Shows the facts about a PDF without changing it: PDF version, page count and sizes, fonts and whether they are embedded, images with effective resolution, encryption and permissions, signature fields, form presence, JavaScript presence, PDF/A claim, attachments, and where the file's bytes go.

## Options

None. Inspect runs when a PDF is added. A password is asked inline, once per file, and kept in memory for the session only.

## How it works

| Fact | Source |
|---|---|
| Version, pages, page sizes and rotation, fonts, images and dpi, signature count, form presence, text layer | PDFium (`engines/pdfium`) |
| Cipher, key length, revision, permission bits | Encrypt dictionary read by lopdf (`engines/lopdf-ops`), readable without the password |
| JavaScript or automatic actions, PDF/A claim from XMP, attachments, byte usage by kind | lopdf object scan |

Both engines run only in the worker process. The app asks through the `inspect` query and gets a `DocumentReport`.

## Behavior and limits

- An encrypted file opened without a password gives a `locked` report (cipher and permissions, no contents) and the Unlock suggestion. A wrong password is `QW-PDF-WRONG-PASSWORD`.
- Fonts and images are read from the first 500 pages. `images.scanned_pages` says how many were read, and the panel notes it for larger files.
- At most 5,000 page sizes are listed; larger documents report the count only.
- lopdf expands streams up to 256 MB while loading, as a decompression bomb guard.
- JavaScript is only detected. Quirewise never runs it.
- The text layer check looks at the first five pages. Pages with images and no text suggest OCR.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PDF-LIMIT-EXCEEDED`, `QW-PATH-DENIED`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0), pdfium-render (MIT or Apache-2.0), lopdf (MIT). No copyleft code.
