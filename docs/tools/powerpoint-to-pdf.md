# PowerPoint to PDF (Appendix A row 20)

## Purpose

Converts presentations to PDF, one slide per page.

## Options

| Option | Type | Meaning |
|---|---|---|
| `slides` | text | slides of the result to keep; default `all` |
| `include_hidden` | switch | include hidden slides; default off |
| `engine` | choice | `auto`, `libre-office` or `ms-office`; default `auto` |

```
qw run powerpoint-to-pdf deck.pptx --options '{"include_hidden":true}'
```

## How it works

PowerPoint saves the deck as PDF; to include hidden slides they are unhidden in memory first (the file is opened read only and never saved). LibreOffice uses its `ExportHiddenSlides` option.

## Behavior and limits

- The slide range counts the slides that are in the result, so hidden slides left out are not counted.

## Errors

`QW-INPUT-NOT-PDF`, `QW-COMPONENT-MISSING`, `QW-OPTION-INVALID`, `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`, `QW-OUTPUT-VERIFY-FAILED`, `QW-PATH-DENIED`.

## License impact

Microsoft Office (not shipped) or LibreOffice (MPL-2.0, on demand). No copyleft code is linked.
