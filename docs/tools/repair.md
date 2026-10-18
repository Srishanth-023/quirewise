# Repair (Appendix A row 15)

## Purpose

Recovers a damaged PDF into a clean copy.

## Options

None.

```
qw run repair broken.pdf
```

## How it works

Two engines are tried in order and every result is verified before it counts. qpdf rewrites the file and rebuilds a broken cross-reference table. If that fails or the result does not verify, PDFium loads the file with its own recovery and saves the pages it could read. The first verified result is published. The input is never changed.

## Behavior and limits

- The file must still start with `%PDF-` in its first bytes, because the app recognizes PDFs by that signature.
- Content that no engine can read is lost. The result keeps the pages that could be recovered.
- A file that opens fine is rewritten anyway.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED` (neither engine recovered it), `QW-PATH-DENIED`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

qpdf (Apache-2.0, separate process), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code.
