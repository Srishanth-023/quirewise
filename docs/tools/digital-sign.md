# Digital signature (Appendix A row 61)

## Purpose

Signs a PDF with a certificate, so that anyone can check who signed and that the file has not changed since. This is the PAdES kind of signature that Adobe Acrobat, Foxit, Okular, Edge and the European validators read. It is not the drawn signature of Sign PDF (row 53), which is only a picture.

The original file is never changed. The signature is appended to a copy as a new revision, so the bytes before it, and any earlier signature, stay exactly as they were.

## Options

| Option | Type | Meaning |
|---|---|---|
| `source` | choice | `pfx` (default): a PFX or P12 file. `windows-store`: a certificate of your personal Windows store |
| `pfx_path` | file | The PFX or P12 file, for `pfx` |
| `pfx_password` | secret | Its password. Used once, kept in memory only, never written to settings or logs |
| `thumbprint` | text | SHA-1 thumbprint (40 hex digits), for `windows-store`. The app offers the certificates that can sign as a list |
| `field` | text | Name of an empty signature field to fill (for example one made with Create forms). Default: a new field is added |
| `visible` | bool | Draw the signature on a page; default off (invisible) |
| `page`, `left`, `top`, `width`, `height` | numbers | Page (from 1) and the box as shares of the displayed page from its top left; default page 1, 0.6, 0.86, 0.35, 0.1 |
| `signer_name` | text | Name to show instead of the one in the certificate |
| `reason`, `location`, `contact` | text | Stored in the signature and drawn in a visible one |
| `level` | choice | `b-b` (default), `b-t`, `b-lt`, `b-lta`, see below |
| `timestamp_url` | text | Address of a time stamp server (RFC 3161), needed from `b-t` up |
| `allow_network` | bool | Consent to the internet connection that `b-t` and up need; default off |
| `certify` | choice | `none` (default, an approval signature), `no-changes`, `form-fill` or `annotate`: certify the document and limit later changes (DocMDP 1, 2 or 3) |

```
qw run digital-sign contract.pdf --options-file sign.json
```

with `sign.json` such as `{"pfx_path":"C:\\keys\\me.pfx","pfx_password":"...","visible":true,"reason":"I approve"}`. Use an options file so the password stays out of the shell history.

## Levels

| Level | What is stored | Needs the internet |
|---|---|---|
| `b-b` | The signature and the certificates of the signer | No |
| `b-t` | Plus a time stamp from a server you name, which proves when the signature existed | Yes: the time stamp server |
| `b-lt` | Plus the revocation answers (OCSP and revocation lists) for the certificates, in the file | Yes: the time stamp and revocation servers |
| `b-lta` | Plus a time stamp over the whole file, so the evidence can be renewed for years | Yes: as `b-lt` |

Levels above `b-b` ask servers, and only after you turned on `allow_network`. What is sent: a hash (of the signature or the file) to the time stamp server, and the certificate number with hashes of its issuer to the revocation servers. The document is never sent. Each connection is logged locally as a kind of event, without the address or the file.

## How it works

1. The certificate and key are read: a PFX file is decrypted in memory (and wiped), or the key stays in Windows and CNG signs the digest, so keys that cannot be exported and keys on smart cards and tokens work. Windows asks for the PIN itself.
2. A revision is appended: the signature dictionary with space reserved for the signature, a signature field with a widget (invisible, or drawn with the name, time, reason, place and contact), and the changed page, form and catalog. A signature field that already exists is filled instead of adding a new one.
3. The SHA-256 (SHA-384 for P-384 keys) of everything except the reserved space is signed. The signature is a CMS structure of the type `ETSI.CAdES.detached` with the content type, the digest and the signing certificate (ESS) as signed attributes. RSA keys use PKCS#1 v1.5; P-256 and P-384 keys use ECDSA.
4. For `b-t` and up, a time stamp of the signature value is requested, checked (it must stamp this signature, repeat the request number and be signed correctly) and stored as an unsigned attribute.
5. For `b-lt` and up, the certificates of the signer and of the time stamp server, and the OCSP answers or revocation lists named in the certificates, are stored in a document security store (DSS) in a further revision. For `b-lta`, a time stamp over the whole file is added in a last revision.
6. Before the file is published it is reopened in PDFium, checked with `qpdf --check`, and its new signature is validated by the signature checker. If the signature does not validate, nothing is written.

## Behavior and limits

- With `field`, the signature goes where that field is and the `visible` box options are ignored. A field with no area gives an invisible signature.
- A password-protected file cannot be signed (`QW-PDF-ENCRYPTED`): protecting after signing would change the signed bytes. Unlock a copy, sign it, and share it as it is.
- A certifying signature (`certify` other than `none`) must be the first signature of the file, and a file certified with no changes allowed accepts no further signature.
- The certificate must be valid today and meant for signing documents. A certificate that is revoked is not used for `b-lt` and `b-lta`.
- Keys: RSA (any size, 2048 or more is advised), ECDSA P-256 and P-384. Other curves and RSA-PSS keys are not used for signing.
- The Windows store lists the personal store of the current user. Keys of older Windows providers (CryptoAPI) are refused with advice to export a PFX; keys of smart cards go through CNG.
- If some revocation answers could not be fetched but others could (for example the server of the time stamp authority names no OCSP address), the file is written and the event `sign.validation-data-incomplete` is logged. If none could be fetched, no file is written.
- A visible signature is drawn in Helvetica; characters outside Western European languages show as `?` in the picture (the signature itself holds the real name).
- Not every viewer shows the status the same way. Acrobat trusts the certificate authorities of its own list (the Adobe Approved Trust List), which can differ from the Windows list.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-DAMAGED` (run Repair first), `QW-OPTION-INVALID` (no certificate, wrong password, invalid box, certificate not valid or not meant for signing, a field that is missing or already signed, certification not possible), `QW-RANGE-OUT-OF-BOUNDS` (page of a visible signature), `QW-NETWORK-CONSENT-REQUIRED`, `QW-NETWORK-UNAVAILABLE`, `QW-COMPONENT-MISSING` (Windows store on another system), `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

RustCrypto crates (`cms`, `x509-cert`, `x509-ocsp`, `der`, `spki`, `rsa`, `p256`, `p384`, `ecdsa`, `sha2`, `sha1`; MIT or Apache-2.0), `p12-keystore` (MIT or Apache-2.0) for PFX files, `rustls-native-certs` (Apache-2.0, ISC or MIT) for the Windows root list, and the `windows` crate (MIT or Apache-2.0) for CNG. The GPL signing crate that ADR-0004 rules out is not used.
