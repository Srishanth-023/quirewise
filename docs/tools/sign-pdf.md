# Sign PDF, visual (Appendix A row 53)

## Purpose

Places a drawn, typed or picture signature, initials, a stamp or a date on the pages of a PDF. This is a picture of a signature. It is not a digital signature and proves nothing about who placed it (see Behavior and limits).

## Options

| Option | Type | Meaning |
|---|---|---|
| `signatures` | list | What to place (at most 2000). At least one is needed. |

Each entry has `page` (from 1), `region` (fractions of the displayed page from the top-left) and a `source`:

| `source.type` | Fields |
|---|---|
| `drawn` | `strokes` (points as fractions of the signature's own box), `color`, `width_pt` |
| `typed` | `text`, `style` (`font`, `installed_font`, `size_pt`, `color`, `bold`, `italic`) |
| `image` | `path` (a PNG or JPEG picture of a signature or a company stamp) |
| `image-data` | `data`: a base64 picture, up to about 11 MB, used for saved pictures |
| `date` | `format` (`iso` 2026-10-19, `long` 19 October 2026, `us` 10/19/2026, `eu` 19/10/2026), `style`. The date is the day the tool runs, read from the clock of the application. |

```
qw run sign-pdf contract.pdf --options '{"signatures":[{"page":3,"region":{"x":0.6,"y":0.8,"width":0.25,"height":0.06},"source":{"type":"typed","text":"Ada Lovelace","style":{"font":"times","italic":true,"size_pt":24}}}]}'
```

In the app, draw, type or choose a picture, then click the page where it goes (or use Add at the center). A signature can be moved and resized before the file is written. Choose "Keep for next time" to save a drawn or typed signature, initials or a stamp. Saved signatures are written to `%LOCALAPPDATA%\Quirewise\signatures` (in the portable build, `data\signatures` next to the program), one `.qwsig` file each, sealed with Windows DPAPI for the current Windows account. They are never uploaded and cannot be read by another account or on another computer.

## How it works

A drawn signature is vector strokes with round joins, scaled to its box. A typed one is written in the chosen font and centered. Pictures are decoded and drawn into the box with their aspect ratio kept. The content is added above the page like the Edit PDF items. Dates are resolved to typed text before the run so the file is deterministic. A password-protected input stays protected.

## Behavior and limits

- **A visual signature is not a digital signature.** It has no certificate, no timestamp and no check that the file was not changed afterwards. Digital signing and verification come with a later phase.
- Picture signatures are not offered for saving, since the file they point to can move; draw or type to keep one.
- Saved signatures are limited to 50. A hostile or damaged `.qwsig` file is ignored.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

`lopdf` (MIT), `image` (MIT or Apache-2.0), `base64` (MIT or Apache-2.0), `windows` (MIT or Apache-2.0, for DPAPI). No copyleft code is linked.
