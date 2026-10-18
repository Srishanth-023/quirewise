# Unlock (Appendix A row 52)

## Purpose

Removes the password and the restrictions from a PDF you can open.

## Options

None. The password you type in the inline prompt when the file is added is used and kept in memory only.

```
qw run unlock locked.pdf
```

Without the password the run fails with `QW-PDF-ENCRYPTED`.

## How it works

qpdf `--decrypt` writes an unprotected copy. The password goes to qpdf on its standard input (`--password-file=-`), never on the command line. The result is verified and published without overwriting. The input is never changed.

## Behavior and limits

- You need the password that opens the file. Unlock does not guess or crack passwords.
- A file without encryption is copied unchanged apart from a rewrite.
- Signatures become invalid because the file changes.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED`, `QW-OUTPUT-VERIFY-FAILED`.

## License impact

qpdf (Apache-2.0, run as a separate process). No copyleft code.
