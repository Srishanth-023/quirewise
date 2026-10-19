# Page numbers (Appendix A row 33)

## Purpose

Writes a page number, or a text with several tokens, in a corner or edge of each page.

## Options

| Option | Type | Meaning |
|---|---|---|
| `pages` | text | `all`, `even`, `odd` or a range; default `all` |
| `template` | text | tokens `{page}`, `{pages}`, `{date}`, `{filename}`; `{{` and `}}` give a brace; default `{page}` |
| `style` | choice | `arabic`, `roman-lower`, `roman-upper`, `letters-lower`, `letters-upper`; default `arabic` |
| `start_number` | number | number on the first numbered page; default 1 |
| `skip_first_page` | switch | no number on page 1 and it is not counted; default off |
| `facing_pages` | switch | mirrors the position (and the horizontal offset) on even pages; default off |
| `position` | choice | nine places on a grid; default `bottom-center` |
| `offset_x_pt, offset_y_pt` | number | move right and up in points; default 0 |
| `font, bold, italic, size_pt, color` | mixed | default Helvetica, 10 pt, black (`#RRGGBB`) |

```
qw run page-numbers report.pdf --options '{"template":"Page {page} of {pages}","position":"bottom-right"}'
```

## How it works

A shared stamper lays the text out on the page as displayed and moves it into the page's own coordinates with one matrix, so rotated pages and pages with an origin offset come out upright and in the right corner. The stamp is wrapped in `q ... Q`, so it cannot change the graphics state of the page. Text uses the standard PDF fonts (Helvetica, Times, Courier; bold and italic), so it needs no embedding. Only Latin-1 and Windows-1252 characters can be drawn; anything else is rejected before the run.

## Behavior and limits

- The text sits 36 pt from the page edge before the offsets are applied.
- The stamp is added to the page content; it is not an annotation and cannot be removed again except by editing the page.
- A password-protected input stays protected: the original password and restrictions are copied to the result (qpdf `--copy-encryption`). A file that opens without a password has no owner restrictions to keep.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

lopdf (MIT), qpdf (Apache-2.0, separate process). No copyleft code.
