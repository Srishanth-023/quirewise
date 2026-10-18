# Protect (Appendix A row 51)

## Purpose

Encrypts a PDF with a password to open it and an optional password to lift restrictions.

## Options

| Option | Type | Meaning |
|---|---|---|
| `user_password` | secret or none | needed to open the file; empty lets anyone open it |
| `owner_password` | secret or none | lifts the restrictions; required when any permission is off |
| `cipher` | `aes256`, `aes128` | default `aes256` |
| `permissions` | `print`, `copy`, `modify`, `annotate`, `fill_forms`, `assemble` | each true or false, all true by default |

Rules: a password is required (at least one of the two), and restrictions need an owner password that differs from the opening password.

Secrets stay out of shell history and logs. Put them in a file and pass it:

```
qw run protect report.pdf --options-file secrets.json
```

where `secrets.json` holds `{"user_password":"...","owner_password":"..."}`. `--options` and `--options-file` cannot be combined.

## How it works

qpdf encrypts the file. The passwords never reach the command line: qpdf reads them from an argument file on its standard input. The passwords live in memory only, are redacted from `Debug` and logs, and travel to the worker over its private stdio pipe. The result is reopened with the password to verify it, and published without overwriting.

## Behavior and limits

- Restrictions are advisory: any viewer may ignore them, and anyone who has the owner password can lift them.
- AES-256 needs a viewer that supports PDF 2.0 or Acrobat 9 and newer. Use `aes128` for very old viewers.
- Protecting an already protected file needs its password first.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OPTION-INVALID`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

qpdf (Apache-2.0, run as a separate process). No copyleft code.
