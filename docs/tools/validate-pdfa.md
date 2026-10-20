# Validate PDF/A (Appendix A row 31)

## Purpose

Checks whether a PDF conforms to PDF/A and writes a report of what it breaks. The file is never changed. The checker is veraPDF.

## Options

| Option | Type | Meaning |
|---|---|---|
| `level` | choice | `1a` to `3u`, or empty for the level the file claims in its metadata; a file that claims none is checked as `1b` |
| `format` | choice | `html` (default): a page to read. `json`: data for other programs |

```
qw run validate-pdfa archive.pdf --options '{"level":"2b","format":"json"}'
```

The report is named `{name} (PDF-A report).html` or `.json`.

## The report

- A sentence that says whether the file conforms and how many rules and places are broken.
- The counts of rules and places that held and that were broken.
- For each broken rule: the clause of the standard, the test number, the rule in the words of the standard, how many places break it and a few examples with where they are and what is wrong.

The JSON form holds the same facts (`profile`, `compliant`, the four counts, `violations` with `specification`, `clause`, `test`, `description`, `count` and `examples`, `validator`).

## Behavior and limits

- A password-protected file is reported as such (`QW-PDF-ENCRYPTED`): PDF/A files cannot be encrypted. Unlock a copy first.
- veraPDF lists at most five failed places for each rule; the count says how many there are.
- veraPDF needs Java. The component brings its own; in a development tree the Java of the computer is used.
- Checking can take a while for large files; it stops after ten minutes.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-DAMAGED`, `QW-COMPONENT-MISSING` (veraPDF or Java), `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`.

## License impact

veraPDF (GPLv3 or MPLv2, used under MPL) runs as a separate Java process; nothing of it is linked.
