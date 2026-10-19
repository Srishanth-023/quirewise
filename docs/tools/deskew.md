# Deskew (Appendix A row 17)

## Purpose

Straightens pages that went into the scanner crooked.

## Options

| Option | Type | Meaning |
|---|---|---|
| `mode` | choice | `auto` (measure each page) or `manual` (use the typed angles); default `auto` |
| `pages` | text | pages to straighten, for auto; default `all` |
| `max_angle_deg` | number | largest tilt corrected automatically, 1 to 45; default 15 |
| `angles` | rows | for manual: a page (from 1) and the turn in degrees, positive counter-clockwise |

```
qw run deskew scan.pdf --options '{"mode":"manual","angles":[{"page":2,"degrees":-1.5}]}'
```

## How it works

The tilt of a page is found from a picture of it: the dark points are projected onto rows at many angles, and the angle that makes the rows sharpest (text lines fall into single rows) wins, first in half-degree steps and then in hundredths. The page content is wrapped in a rotation matrix about the center of the page as displayed, so pictures and text keep their quality and the page keeps its size.

## Behavior and limits

- A page with too little to go by, or without clear lines, is left alone. Tilts under a quarter degree are not corrected.
- When no page needs turning the run stops with a message and saves nothing.
- Annotations and form fields stay where they were; the corners the turned page no longer fills show the page background.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`. A missing component is `QW-COMPONENT-MISSING`.

## License impact

PDFium (BSD-3-Clause and Apache-2.0), lopdf (MIT). No copyleft code is linked.
