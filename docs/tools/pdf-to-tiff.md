# PDF to TIFF (Appendix A row 68)

## Purpose

Writes pages as one multi-page TIFF file.

## Options

| Option | Type | Meaning |
|---|---|---|
| `compression` | choice | `lzw`, `deflate` or `ccitt-g4`; default `lzw` |
| `dpi` | number | 10 to 2400; default 150 |
| `color` | choice | `color`, `gray` or `black-white`; CCITT G4 always writes black and white; default `color` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |

```
qw run pdf-to-tiff contract.pdf --options '{"compression":"ccitt-g4","dpi":300}'
```

## How it works

Pages are rendered one at a time and appended to the file as they are made. The writer is part of Quirewise because Group 4 compression is not offered by the usual TIFF encoders. Pages are black and white after the Otsu threshold when `ccitt-g4` or `black-white` is chosen.

## Behavior and limits

- The finished file is opened again and its frames are counted before it is saved.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0), `fax` (MIT), `weezl` (MIT or Apache-2.0), flate2 (MIT or Apache-2.0). No copyleft code is linked.
