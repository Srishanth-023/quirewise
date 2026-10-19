# Crop (Appendix A row 37)

## Purpose

Cuts pages down to a box: fixed margins, a drawn region, or the content itself.

## Options

| Option | Type | Meaning |
|---|---|---|
| `method` | choice | `margins`, `region` or `auto`; default `margins` |
| `margins, unit` | group | top, right, bottom, left in `mm`, `in` or `pt`; default millimeters |
| `region` | box | drawn on the first page in the UI, stored as fractions of the displayed page |
| `auto_padding_mm` | number | space kept around the content for `auto`; default 2 |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |
| `apply_to_media_box` | switch | also cut the media box so other programs see the smaller page; default off |

```
qw run crop scan.pdf --options '{"method":"margins","margins":{"top":10,"right":10,"bottom":10,"left":10},"unit":"mm"}'
```

## How it works

Sets the CropBox (and the MediaBox when asked) to the box, converted from the displayed page back to the page's own coordinates, so rotated pages are cut where you see the cut. `auto` renders each page at low resolution and keeps the area that is not paper white.

## Behavior and limits

- Without `apply_to_media_box` the content outside the box is still in the file; a viewer only hides it. Turn the switch on, or flatten, when the hidden part must be gone.
- Margins that cut away the whole page are rejected.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
