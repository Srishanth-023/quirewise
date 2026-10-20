# Fill and sign (Appendix A row 45)

## Purpose

Fills the fields of an interactive form and places signatures, initials, stamps and dates in the same run.

## Options

| Option | Type | Meaning |
|---|---|---|
| `values` | list | `{ "name": "...", "value": "..." }` for each field to fill. The name is the field's full name (parents joined by dots). A check box takes `true` or `false`; a radio button takes the choice it stands for; a drop-down takes one of its choices. |
| `signatures` | list | Signatures, initials, stamps and dates, as in [Sign PDF](sign-pdf.md). |

At least one value or one signature is needed.

```
qw run fill-sign application.pdf --options '{"values":[{"name":"applicant.name","value":"Ada Lovelace"},{"name":"agree","value":"true"}],"signatures":[]}'
```

In the app, the fields of the page appear as inputs over the preview; type, tick or choose. The signature tabs are the same as for Sign PDF. The list of fields comes from the file itself (`form_fields`), so a file with no form says so and still takes signatures.

## How it works

The form is read in `lopdf`: the field tree is walked with inherited properties (type, value, options, flags), and widgets give the page and place. A value is written into the field and an appearance stream is generated for each widget (text with the field's font, size and alignment; check marks; radio dots), then `NeedAppearances` is set so other readers redraw from the values as well. Signatures are added after the values. A password-protected input is unlocked, filled and protected again.

## Behavior and limits

- A value for a field that does not exist, or a choice the field does not have, stops the run with a message and writes nothing.
- Read-only fields cannot be filled.
- Form JavaScript (calculations, formats and validations written in the file) is not run. A calculated field keeps what you typed.
- Signature fields are not signed digitally here; a visual signature can be placed over them. Digital signature (`docs/tools/digital-sign.md`) can fill an empty signature field with a real signature.
- Barcode and rich-text fields are left as they are.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-RANGE-OUT-OF-BOUNDS`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

`lopdf` (MIT). No copyleft code is linked.
