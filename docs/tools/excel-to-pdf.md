# Excel to PDF (Appendix A row 21)

## Purpose

Converts workbooks to PDF.

## Options

| Option | Type | Meaning |
|---|---|---|
| `sheets` | text | sheets to print, by position, for example `1,3`; default `all` |
| `fit_to_page_width` | switch | shrink wide sheets to the page width; default on |
| `orientation` | choice | `auto` (what each sheet has), `portrait` or `landscape`; default `auto` |
| `engine` | choice | `auto`, `libre-office` or `ms-office`; default `auto` |

```
qw run excel-to-pdf budget.xlsx --options '{"sheets":"1-2","orientation":"landscape"}'
```

## How it works

With Microsoft Office the page setup of each sheet is changed in memory before the export. With LibreOffice, which cannot do that from its command line, the settings are written into a temporary copy of the `.xlsx` file: unwanted sheets are hidden (hidden sheets are not printed), and every sheet gets fit-to-width and the page turn. The original is never changed.

## Behavior and limits

- With LibreOffice, choosing sheets needs an `.xlsx` file; for `.xls` and `.ods` files use Microsoft Office. Fit and orientation are applied to `.xlsx` files only.
- Sheet numbers that do not exist are ignored; if none exist the run stops with a message.

## Errors

`QW-INPUT-NOT-PDF`, `QW-COMPONENT-MISSING`, `QW-OPTION-INVALID`, `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`, `QW-OUTPUT-VERIFY-FAILED`, `QW-PATH-DENIED`.

## License impact

Microsoft Office (not shipped) or LibreOffice (MPL-2.0, on demand). No copyleft code is linked.
