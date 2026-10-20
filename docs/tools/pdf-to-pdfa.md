# PDF to PDF/A (Appendix A row 30)

## Purpose

Writes a copy of a PDF that follows PDF/A, the standard for documents that must look the same decades from now. The original is never changed. Every result is checked with veraPDF before it is saved, so a file that is saved conforms to the level in its name.

## Options

| Option | Type | Meaning |
|---|---|---|
| `level` | choice | `1a`, `1b`, `2a`, `2b` (default), `2u`, `3a`, `3b` or `3u` |
| `allow_downgrade` | bool | When the level cannot be reached, try the next weaker one; default on |
| `rasterize_if_needed` | bool | Last resort: replace the pages by pictures so the file conforms; default off |

The numbers are parts of the standard: 1 is built on PDF 1.4 (no transparency, no layers), 2 on PDF 1.7, 3 on PDF 1.7 and allows any attached file. The letters are what is promised: `b` the pages look the same, `u` also that the text can be copied right, `a` also that the document is tagged for accessibility.

```
qw run pdf-to-pdfa report.pdf --options '{"level":"2b"}'
```

The result is named `{name} (PDF-A {level}).pdf` with the level that was reached, which may be weaker than the one asked for.

## How it works

1. A password-protected file is unlocked into a scratch copy (PDF/A files are never encrypted).
2. The document is normalized:
   - Fonts that are not in the file are embedded from the fonts installed on the computer: Helvetica as Arial, Times as Times New Roman, Courier as Courier New, and fonts by their own name when they are installed (Calibri, Verdana, ...). Whole TrueType files are embedded.
   - An sRGB color profile is added as the output intent (the profile is generated here from the published sRGB numbers, so it carries no license), unless the file brings an output intent with a profile of its own.
   - An XMP packet that claims the level is written. The document information is rewritten to agree with it exactly, with dates in UTC; the producer is set to Quirewise when the file names none.
   - JavaScript, actions that run programs or sounds, attachments (parts 1 and 2), page transitions, thumbnails and private application data are removed. Annotations get the print flag and lose forbidden flags, actions and appearance states; annotations that cannot hold (no appearance, sound, movie) are removed. Pictures lose "smooth when enlarged", transfer functions and halftones are removed, LZW streams are recompressed, references to external files are removed, layers are removed in part 1 and get names in parts 2 and 3.
   - The file gets an ID, a plain cross-reference table and the header of the part (1.4 or 1.7).
   - For levels `u` and `a`, embedded fonts get a Unicode map.
3. veraPDF checks the result against the level. If it conforms, it is saved.
4. If not, and downgrading is allowed, the next weaker level is tried: `1a` then `1b` then `2b`; `2a` then `2u` then `2b`; `3a` then `3u` then `3b`; `1b` then `2b`.
5. With the Ghostscript plugin installed (see below) it is tried next, for the same levels.
6. With `rasterize_if_needed`, every page is drawn as a 200 dpi picture, the pictures are made into a document and normalized. This conforms, and the text is no longer selectable.
7. If nothing conforms, no file is written and the message names the rules that are still broken.

## Behavior and limits

- **Level A is not reached.** Tags (the structure that screen readers follow) cannot be added without understanding the document, so `1a`, `2a` and `3a` fall back to the `u` or `b` level, or fail without downgrade. A file that is already tagged keeps its tags and may conform at level A.
- **Color.** Device RGB and gray are covered by the sRGB output intent. Device CMYK is not converted: such a file does not conform unless it has its own CMYK output intent, and needs `rasterize_if_needed` or another program.
- **Fonts.** A font that is not installed cannot be embedded, and neither can fonts with custom encodings, symbol fonts or CID fonts that are not in the file. Embedding replaces the look of the font by the installed one with the same measurements. Whole fonts are embedded, so files grow (Arial is about 0.4 MB compressed).
- **Transparency** stays: part 1 does not allow it, so such a file conforms at part 2 (downgrade), or as pictures.
- **Signatures** are not kept valid: the file is rewritten, so earlier signatures stop validating.
- Pictures and the look of the pages are not changed, except in the last-resort mode.
- veraPDF is needed (an on-demand component). Without it the tool says so and offers the Components screen.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-COMPONENT-MISSING` (veraPDF), `QW-OUTPUT-VERIFY-FAILED` (the file could not be made to conform; the message lists the broken rules), `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`.

## License impact

lopdf and ttf-parser as before. veraPDF (GPLv3 or MPLv2, used under MPL) runs as a separate Java process. Ghostscript (AGPL-3.0) is only ever run as a separate process from a build with the `agpl-ghostscript` feature, never linked, never in the default build.
