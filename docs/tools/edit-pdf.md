# Edit PDF (Appendix A row 42)

## Purpose

Adds text, pictures, shapes, links and patches to the pages of a PDF, and changes existing text. The original file is never changed; the result is a new file.

## Options

| Option | Type | Meaning |
|---|---|---|
| `items` | list | Everything to add, in drawing order (at most 2000). At least one is needed. |

Each item has a `kind` and a `page` (from 1). Positions are fractions of the displayed page, measured from the top-left corner (`region`: `x`, `y`, `width`, `height`, each from 0 to 1), so a rotated page needs no special care.

| `kind` | Fields |
|---|---|
| `text` | `text` (up to 20000 characters), `region`, `style` (`font`: `helvetica`, `times` or `courier`; or `installed_font`: a family from Windows; `size_pt` 4 to 400; `color` as `#RRGGBB`; `bold`; `italic`), `align` (`left`, `center`, `right`) |
| `image` | `path` (PNG or JPEG), `region` |
| `shape` | `shape` (`rectangle`, `ellipse`, `line`, `arrow`), `region`, `stroke_color`, `stroke_width_pt`, `fill_color` (rectangle and ellipse), `flip` (line and arrow: runs from the bottom-left corner to the top-right) |
| `link` | `region`, `target`: `{"type":"uri","uri":"https://..."}` (http, https or mailto only) or `{"type":"page","page":3}` |
| `whiteout` | `region`, `color` (default white): a patch over what is under it |

```
qw run edit-pdf form.pdf --options '{"items":[{"kind":"text","page":1,"region":{"x":0.1,"y":0.1,"width":0.4,"height":0.05},"text":"Approved","style":{"size_pt":14,"bold":true}}]}'
```

In the app, choose the tool, pick what to place and drag on the page. Items can be moved, resized (pointer or the number fields and arrow keys), changed in the details panel, deleted and undone. **Change text** shows the blocks of text of the page; picking one puts a patch over it and new text in the same place, ready to edit.

## How it works

The new content is drawn above the page content inside `q ... Q`, with a matrix that maps the displayed page (including its rotation and crop box) to the page's own space. Text is wrapped inside its box. Standard fonts are the built-in Helvetica, Times and Courier families (with bold and italic) in WinAnsi encoding. An installed font (plain TrueType with outlines, from the Windows or user font folder) is embedded whole as a simple WinAnsi font; a font whose license bits forbid embedding is refused. Links are real `Link` annotations. Pictures are decoded once and shared between items that use the same file. A password-protected input is unlocked, edited and protected again with the same settings.

## Behavior and limits

- **A patch is not a redaction.** The covered text is still in the file and can be found and copied. Use Redact (a later phase) to remove content for good.
- Only characters in WinAnsi (Western European) and that the chosen font has can be written. For anything else the run stops with a message that names the character, and writes nothing.
- Fonts are embedded whole, not subset, so a large font makes a larger file.
- OpenType fonts with CFF outlines and font collections are not offered.
- Only PNG and JPEG pictures are read, and SVG is not supported yet.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

`lopdf` (MIT), `ttf-parser` (MIT or Apache-2.0), `image` (MIT or Apache-2.0). No copyleft code is linked.
