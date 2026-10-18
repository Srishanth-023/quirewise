# Remove annotations (Appendix A row 44)

## Purpose

Deletes comments and marks from pages: highlights, strikeouts, sticky notes and links.

## Options

| Option | Type | Meaning |
|---|---|---|
| `kinds` | list of `all`, `highlights`, `strikeouts`, `notes`, `links` | what to remove; default `all` |
| `pages` | page range | `all`, `even`, `odd` or a range such as `1,3-5`; default `all` |

`all` removes every annotation except form fields and signatures.

```
qw run remove-annotations review.pdf --options '{"kinds":["highlights","notes"],"pages":"1-3"}'
```

## How it works

The page's `/Annots` array is filtered by annotation subtype, and the removed objects, including popups, are pruned from the file. The work uses lopdf (not PDFium, see open question 24). An encrypted file is decrypted into a scratch copy first and the result is repacked by qpdf. Output is verified and published without overwriting.

## Behavior and limits

- A password-protected input gives an unprotected result: the file is decrypted with the password you entered and written without encryption. Run Protect afterwards to lock it again.
- Form fields and signature fields are never removed, so filled forms and signed files keep working. Use Flatten to bake form fields into the page.
- Text that was highlighted keeps its look in the page content; only the highlight mark goes.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-RANGE-SYNTAX`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
