# Word to PDF (Appendix A row 19)

## Purpose

Converts Word, OpenDocument text and RTF files to PDF.

## Options

| Option | Type | Meaning |
|---|---|---|
| `pages` | text | pages of the result to keep: `all`, a range, `even`, `odd`; default `all` |
| `engine` | choice | `auto`, `libre-office` or `ms-office`; default `auto` |

```
qw run word-to-pdf letter.docx --options '{"pages":"1-2","engine":"ms-office"}'
```

## How it works

Microsoft Office is driven through Windows PowerShell and COM: a hidden instance opens the file read only with macros off and exports a PDF. Without Office, LibreOffice converts in headless mode with its own empty profile. The page range is applied to the finished PDF.

## Behavior and limits

- Automatic uses Microsoft Office when it is installed, otherwise LibreOffice. LibreOffice is a component: install it in Settings, Components.
- Documents that ask for a password cannot be opened without it; the file fails with a message, or stops at the time limit.
- Layout can differ slightly between Office and LibreOffice.

## Errors

`QW-INPUT-NOT-PDF`, `QW-COMPONENT-MISSING`, `QW-OPTION-INVALID`, `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`, `QW-OUTPUT-VERIFY-FAILED`, `QW-PATH-DENIED`.

## License impact

Microsoft Office (the person's own licence, not shipped) or LibreOffice (MPL-2.0, downloaded on demand). No copyleft code is linked.
