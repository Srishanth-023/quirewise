# Compare (Appendix A row 55)

## Purpose

Shows what differs between two versions of a document. The result is a PDF report with a summary, the text changes of the two files side by side, and a picture of every page that looks different. The two files are never changed.

## Inputs and options

Two PDF files are needed: the original first, then the changed one. Each can have its own password.

| Option | Type | Meaning |
|---|---|---|
| `mode` | choice | `both` (default), `text` or `visual` |
| `tolerance_percent` | number | 0 to 50, default 2. A pixel counts as changed when a color channel differs by more than this share of the full range. |
| `ignore_whitespace` | bool | Runs of spaces and tabs count as one space and blank lines are ignored; default on |
| `pages` | text | Pages to compare, by number in both files: `all`, `even`, `odd` or a range; default `all` |
| `dpi` | integer | 36 to 200, default 100: resolution the pages are compared at |

```
qw run compare old.pdf new.pdf --options '{"mode":"both","tolerance_percent":2}'
```

## How it works

- **Text.** The text of each compared page is read with PDFium and split into lines. All lines of a file are compared with the lines of the other (Myers' algorithm), so text that moves to another page because something was added earlier is not reported as changed. A line that was rewritten is shown beside its new form, with the page of each. Unchanged text appears only as one line of context around a change.
- **Look.** Each pair of pages is rendered and compared pixel by pixel. If the pages are not the same size they are laid on one canvas by their top left corners, and the report says so. A page that is missing in one file counts as blank paper there. Pages with at least one changed pixel get a picture: unchanged parts faded, ink only in the original red, ink only in the changed file blue, and the share of changed pixels.
- **The report** is written on Letter pages: a summary page (names, page counts, how many lines changed, which pages differ and by how much), the text changes in two columns with a color behind removed and added lines, then one page for each page that looks different. Output is named `{original} (compared).pdf`.

## Behavior and limits

- Text is compared line by line, not word by word: a rewritten line is shown whole.
- The report is drawn in the standard fonts, so characters outside Western European languages show as `?` in the report (the comparison itself uses the real text).
- The report lists up to 1500 changed rows; the number of the rest is stated. Compare fewer pages to see them.
- Pages are compared by number. A page inserted early makes every later page differ in the look comparison; the text comparison is not affected.
- Scanned pages have no text; compare their look.
- The report is not protected, even when the inputs were: it holds text from both files.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID` (not two files, a number out of range), `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

`similar` (Apache-2.0), PDFium (BSD-3-Clause and Apache-2.0). No copyleft code is linked.
