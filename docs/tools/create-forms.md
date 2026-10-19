# Create forms (Appendix A row 46)

## Purpose

Adds fillable fields to a PDF: text, check boxes, radio buttons, drop-down lists and signature fields. Finding the places for fields from the page is offered as a suggestion; nothing is added until the person accepts it.

## Options

| Option | Type | Meaning |
|---|---|---|
| `fields` | list | The fields to add (at most 2000). At least one is needed. |

Each field has `page` (from 1), `region` (fractions of the displayed page from the top-left), a unique `name` (up to 200 characters), optional `tooltip` and `required`, and a `kind`:

| `kind.type` | Fields |
|---|---|
| `text` | `multiline`, `value` |
| `checkbox` | `checked` |
| `radio` | `option`: the name of this choice. Fields with the same `name` form one group. |
| `dropdown` | `options` (the choices), `value` |
| `signature` | none: an empty field for a signature |

```
qw run create-forms letter.pdf --options '{"fields":[{"page":1,"region":{"x":0.2,"y":0.2,"width":0.4,"height":0.04},"name":"full_name","kind":{"type":"text"}}]}'
```

In the app, pick a field type and drag on the page. **Find fields** looks at the page for underlines and empty boxes next to words and shows each as a green outline; click one to add it or choose Add all. Names come from the words next to the place ("Name" gives `Name`) and are made unique.

## How it works

Fields are written into the document's `AcroForm`: a field dictionary and a widget annotation for each, with a generated appearance and default appearance string (Helvetica) so every reader draws them, and `NeedAppearances` set. Radio buttons with one name share a parent with a choice for each widget. Names are checked against the fields already in the file, and a name that is taken stops the run. Finding fields is pure code on the words and lines that PDFium reports: a line of text followed by a ruled line or an empty box on the same row suggests a text field, a small empty square next to words a check box. Each suggestion keeps the label it came from.

## Behavior and limits

- Detection is a heuristic and may miss fields or suggest too many; every suggestion is confirmed by the person.
- Scripts, formats, calculations and validation are not created.
- Signature fields are empty containers. They are not digitally signed.
- A file that already has a form keeps it; the new fields are added to it.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

`lopdf` (MIT). No copyleft code is linked.
