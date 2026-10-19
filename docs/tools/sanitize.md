# Sanitize (Appendix A row 63)

## Purpose

Removes hidden or risky content before a file is shared: metadata, scripts, attachments, hidden layers and application data.

## Options

All booleans, all on by default.

| Option | Removes |
|---|---|
| `metadata` | the document information (title, author, dates) |
| `xmp` | the XMP metadata packets |
| `javascript` | JavaScript, `/AA` and `/OpenAction` actions |
| `embedded_files` | attached files, including file attachment annotations |
| `hidden_layers` | layers that are off, with the content marked for them |
| `form_actions` | actions of form fields and of links that run or open something |
| `thumbnails` | page thumbnails |
| `private_data` | `PieceInfo` data that applications stored |

```
qw run sanitize contract.pdf --options '{"embedded_files":false}'
```

## How it works

lopdf edits the object graph and prunes what became unreachable, so removed data is gone from the file and not only hidden. Hidden layers are cut out of the page content (`BDC /OC ... EMC`) and their resources dropped. An encrypted file is decrypted into a scratch copy first and repacked by qpdf. Output is verified and published without overwriting.

## Behavior and limits

- Hidden layers are removed only when the marked content sits in a page content stream, not inside form XObjects (see open question 27).
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.
- Visible text and images are never touched. Sanitize does not redact; use Redact for that.
- Signatures become invalid.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
