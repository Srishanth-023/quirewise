# Watermark (Appendix A row 36)

## Purpose

Puts a text or a picture on pages, above or below the content, once or repeated across the page.

## Options

| Option | Type | Meaning |
|---|---|---|
| `kind` | choice | `text` or `image`; default `text` |
| `text` | text | the text, for a text watermark |
| `image_path` | path | a PNG or JPEG file, for a picture watermark; the UI offers a file chooser |
| `image_width_percent` | number | width of the picture as a percent of the page width, 1 to 100; default 50 |
| `pages` | text | default `all` |
| `position, offset_x_pt, offset_y_pt` | mixed | default `center` |
| `tile` | switch | repeat across the page; default off |
| `rotation_deg` | number | counter-clockwise, 0 to 360; default 0 |
| `transparency` | number | 1 to 100; default 50 |
| `layer` | choice | `above` or `below` the page content; default `above` |
| `font, bold, italic, size_pt, color` | mixed | default Helvetica, 48 pt, gray |

```
qw run watermark draft.pdf --options '{"text":"DRAFT","rotation_deg":45,"transparency":70}'
```

## How it works

The mark is a form with an extended graphics state that carries the opacity. A mark below the content is written before the page content, one above after it. Text uses the standard PDF fonts (Helvetica, Times, Courier; bold and italic), so it needs no embedding. Only Latin-1 and Windows-1252 characters can be drawn; anything else is rejected before the run. Pictures are decoded, limited in size and embedded as RGB with their transparency kept as a soft mask.

## Behavior and limits

- A mark below the content is hidden by pages that paint a solid background.
- The picture path is read by the worker; it must be a local file you chose.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process), the `image` crate (MIT or Apache-2.0), PDFium for the fallback (BSD-3-Clause and Apache-2.0). No copyleft code.
