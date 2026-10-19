# TIFF to PDF (Appendix A row 68)

## Purpose

Turns every frame of a TIFF file into a page.

## Options

| Option | Type | Meaning |
|---|---|---|
| `(none)` | - | the pages follow the frames of the file |

```
qw run tiff-to-pdf scan.tif
```

## How it works

The TIFF decoder reads each frame (uncompressed, LZW, Deflate, PackBits, CCITT G3 and G4, JPEG). Black and white frames are stored as 1-bit images, which keeps scans small. The page size comes from the resolution in the file, or 150 dpi when the file has none.

## Behavior and limits

- Frames in 16 bits per sample are reduced to 8; floating point and Lab frames are not supported and say so.
- At most 5000 frames per file.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

The `tiff` crate (MIT), `weezl` (MIT or Apache-2.0), `fax` (MIT), lopdf (MIT). No copyleft code is linked.
