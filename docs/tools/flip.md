# Flip pages (Appendix A row 39)

## Purpose

Mirrors pages left to right or top to bottom.

## Options

| Option | Type | Meaning |
|---|---|---|
| `direction` | choice | `horizontal` or `vertical`; default `horizontal` |
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |

```
qw run flip scan.pdf --options '{"direction":"horizontal","pages":"2-4"}'
```

## How it works

A mirror matrix is applied to the whole page content (wrapped in `q ... Q`), around the center of the displayed page.

## Behavior and limits

- Annotations and form fields stay where they were, so they no longer sit on what they marked.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
