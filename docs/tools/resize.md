# Resize pages (Appendix A row 38)

## Purpose

Gives pages a standard or custom size, scaling the content to fit or centering it.

## Options

| Option | Type | Meaning |
|---|---|---|
| `size` | choice | `a4`, `letter`, `legal`, `a3` or `custom`; default `a4` |
| `custom_width_mm, custom_height_mm` | number | for `custom`; default 210 by 297 |
| `margin_mm` | number | empty space around the content; default 0 |
| `scale_content` | switch | fit the content to the new page; off keeps its size, centered; default on |
| `match_orientation` | switch | turn the target to match each page, so landscape stays landscape; default on |

```
qw run resize drawing.pdf --options '{"size":"a3","margin_mm":10}'
```

## How it works

Each page's content is wrapped in a form and placed on a new page box with one matrix. Scaling keeps the aspect ratio.

## Behavior and limits

- Annotations (links, comments) and form fields are not moved or scaled with the content. Flatten them first for exact results.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
