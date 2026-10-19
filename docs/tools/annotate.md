# Annotate (Appendix A row 43)

## Purpose

Adds highlights, underlines, strike-outs, sticky notes and freehand drawings as real PDF annotations that other readers show and let people edit.

## Options

| Option | Type | Meaning |
|---|---|---|
| `annotations` | list | The marks to add (at most 2000). At least one is needed. |

Each annotation has a `kind` and a `page` (from 1). Regions and points are fractions of the displayed page from the top-left corner.

| `kind` | Fields |
|---|---|
| `highlight`, `underline`, `strikeout` | `regions` (one or more boxes, such as one for each line), `color` (`#RRGGBB`; defaults yellow, red, red) |
| `note` | `at` (a point), `text`, `color` |
| `freehand` | `strokes` (lists of points, up to 500 strokes and 20000 points), `color`, `width_pt` |

```
qw run annotate report.pdf --options '{"annotations":[{"kind":"highlight","page":2,"regions":[{"x":0.1,"y":0.3,"width":0.5,"height":0.02}]}]}'
```

In the app, pick a mark and drag over the text (highlight, underline and strike-out), click for a note, or draw. Each mark can be selected, moved, recolored and deleted before the file is written.

## How it works

Every annotation gets its geometry (`QuadPoints` for text marks, an ink list for drawings), a color, flags and a generated appearance stream, so it shows the same in every reader. Highlights use the Multiply blend mode so the text stays readable. Notes are `Text` annotations with the print and no-zoom flags set; their text is the popup content. Existing annotations are kept as they are. A password-protected input stays protected.

## Behavior and limits

- Marks are placed by position, not by selecting text: a highlight covers the box you drew, even where the text runs differently.
- The text of a note is kept in full, including characters outside Western European languages.
- Reply threads and popups of other readers are not created.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

`lopdf` (MIT). No copyleft code is linked.
