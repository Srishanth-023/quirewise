# Accessibility check, PDF/UA (Appendix A row 70)

## Purpose

Checks whether a PDF follows PDF/UA-1 (ISO 14289-1), the standard that lets screen readers and other assistive programs use a document: tags, reading order, alternative text, language, and the way fonts and text are stored. The file is never changed. The checker is veraPDF.

## Options

| Option | Type | Meaning |
|---|---|---|
| `format` | choice | `html` (default): a page to read. `json`: data for other programs |

```
qw run check-pdfua brochure.pdf
```

The report is named `{name} (accessibility report).html` or `.json` and has the same form as the PDF/A report (see `validate-pdfa.md`), with the rules of PDF/UA-1.

## Behavior and limits

- veraPDF checks what can be checked by machine. A file that conforms can still be hard to use (poor alternative text, a confusing reading order); people must review those.
- Most files made without tagging fail the first rules (no tags, no language, no title). Quirewise does not tag documents: fix the file in the program that made it (Word, InDesign, LibreOffice) and export with accessibility turned on.
- The limits of Validate PDF/A apply: password-protected files must be unlocked first, at most five failed places are listed for each rule, and veraPDF and Java are needed.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-DAMAGED`, `QW-COMPONENT-MISSING` (veraPDF or Java), `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`.

## License impact

veraPDF (GPLv3 or MPLv2, used under MPL) runs as a separate Java process; nothing of it is linked.
