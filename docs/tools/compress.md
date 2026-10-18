# Compress (Appendix A row 14)

## Purpose

Makes a PDF smaller by recompressing and downsampling page images and repacking the file structure.

## Options

| Option | Type | Meaning |
|---|---|---|
| `preset` | `extreme`, `recommended`, `less` | JPEG quality 40 and 72 ppi, 65 and 144 ppi, or 80 and 288 ppi; default `recommended` |
| `jpeg_quality` | 1 to 100 or none | replaces the preset's quality |
| `max_image_ppi` | `72`, `144`, `288`, `720` or none | replaces the preset's resolution |
| `grayscale` | boolean | turn color images gray, default off |
| `strip_metadata` | boolean | remove the document information and XMP, default off |
| `object_streams` | boolean | pack objects into compressed streams, default on |

```
qw run compress scan.pdf --options '{"preset":"extreme","grayscale":true}'
```

## How it works

The raster engine walks each page's content stream to learn how large every image is drawn (it follows `q`, `Q`, `cm` and `Do`), computes the pixels per inch, and downsamples images above the target. Images are re-encoded as JPEG and the new image replaces the old one only when it is smaller. qpdf then rewrites the file with object streams and recompressed streams. The result is verified before it is published, and the input is never changed.

## Behavior and limits

- Only images drawn directly on pages are touched, and only 8-bit gray and RGB images stored as DCT (JPEG) or Flate. Masks, indexed, CMYK and 16-bit images are left as they are.
- An image is never made bigger. A file with nothing to gain is still published, rewritten.
- A password-protected input gives an unprotected result: the file is decrypted with the password you entered and written without encryption. Run Protect afterwards to lock it again.
- Fonts are not subset (see open question 23).

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), the `image` crate (MIT or Apache-2.0), qpdf (Apache-2.0, separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
