# Edit metadata (Appendix A row 48)

## Purpose

Sets, changes or removes the document information: title, author, subject, keywords, creator, producer, dates and the trapped flag.

## Options

| Option | Type | Meaning |
|---|---|---|
| `title`, `author`, `subject`, `keywords`, `creator`, `producer` | text or none | a value sets the field; an empty text removes it; none keeps it |
| `creation_date`, `modification_date` | text or none | `2026-10-19` or `2026-10-19T14:30:00`, same rules |
| `trapped` | `true`, `false`, `unknown` or none | the Trapped flag |
| `remove_all` | boolean | remove all metadata, default off |

The form shows a "Remove" switch next to each text. It sends a single space, which the core treats as blank and so removes the field.

```
qw run edit-metadata report.pdf --options '{"title":"Annual report","author":""}'
```

## How it works

lopdf rewrites the Info dictionary and the XMP packet so that the two agree. An encrypted file is decrypted into a scratch copy first and repacked by qpdf. Output is verified and published without overwriting.

## Behavior and limits

- The form does not prefill the current values (see open question 26).
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.
- The XMP packet is rewritten, so a PDF/A claim in it is lost.
- Dates must parse; a bad date is rejected before any work starts.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
