# Grayscale (Appendix A row 41)

## Purpose

Turns colors into shades of gray.

## Options

| Option | Type | Meaning |
|---|---|---|
| `apply_to` | choice | `both` (pictures and text), `images` or `text`; default `both` |
| `raster_fallback` | switch | turn every page into a gray picture instead; default off |
| `dpi` | choice | resolution of that picture: `75`, `100`, `150` or `220`; default 150 |

```
qw run grayscale colorful.pdf --options '{"apply_to":"both"}'
```

## How it works

Pictures: gray and RGB pictures (and soft masks) are decoded and re-encoded in gray. Content: color operators (`rg`, `RG`, `k`, `K`, `sc`, `scn` and the device color spaces) are rewritten to gray with the luminance formula. With `raster_fallback` every page is rendered in gray and rebuilt as a picture, the same way Flatten does for "everything".

## Behavior and limits

- Without the fallback, shadings, patterns, inline images and CMYK pictures keep their color. Use the fallback when every color must go.
- The fallback makes text unselectable and the file larger.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process), the `image` crate (MIT or Apache-2.0), PDFium for the fallback (BSD-3-Clause and Apache-2.0). No copyleft code.
