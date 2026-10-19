# Attachments (Appendix A row 65)

## Purpose

Lists, adds, extracts and deletes the files attached to a PDF.

## Options

| Option | Type | Meaning |
|---|---|---|
| `action` | choice | `list`, `add`, `extract` or `delete`; default `list` |
| `files` | paths | for `add`: one path per line; the UI offers a file chooser |
| `names` | names | for `extract` (none means all) and `delete`: one name per line |

```
qw run attachments report.pdf --options '{"action":"extract","names":["data.csv"]}'
```

## How it works

`list` writes a text file with the name, size and date of each attachment. `add` embeds the files, `delete` removes the entries and prunes their data, `extract` writes each attachment as its own file named after the PDF and the attachment. Extracted names are made safe and get a harmless extension; files are never written outside the output folder.

## Behavior and limits

- Attachments are not scanned or opened; treat extracted files as untrusted.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
