# Linearize (Appendix A row 64)

## Purpose

Writes a PDF for fast web view, so the first page shows before the whole file has downloaded.

## Options

| Option | Type | Meaning |
|---|---|---|
| `enabled` | boolean | default true; false writes a normal, non-linearized copy |

```
qw run linearize catalog.pdf
```

## How it works

qpdf `--linearize` reorders the objects. The result is checked with `qpdf --check-linearization` and the page count is compared before it is published, without overwriting. The input is never changed.

## Behavior and limits

- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.
- The result is rewritten with compressed object streams and recompressed streams, so the size can change.
- Signatures become invalid because the file changes.
- An encrypted input needs its password.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

qpdf (Apache-2.0, run as a separate process). No copyleft code.
