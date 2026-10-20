# Verify signatures (Appendix A row 62)

## Purpose

Checks the digital signatures of a PDF and writes a PDF report that says, for each signature, who signed, whether the signed part of the file is intact, whether the file changed afterwards, whether the signer's certificate leads to an authority that is trusted, whether a certificate was revoked, and what a time stamp proves. The file is never changed.

## Options

| Option | Type | Meaning |
|---|---|---|
| `trusted_certificates` | file | A file of certificates (PEM or DER, one or many) to trust besides those of Windows, for example the root certificate of your company. Default none |
| `check_revocation` | bool | Ask the certificate authorities named in the certificates whether a certificate was revoked. Needs `allow_network`. Default off |
| `allow_network` | bool | Consent to the internet connection for the revocation check. Default off |

```
qw run verify-signatures contract.pdf --options '{"trusted_certificates":"C:\\certs\\company-root.pem"}'
```

The report is named `{name} (signature report).pdf`.

## What each signature is checked for

| Check | Passes when | Fails when |
|---|---|---|
| Signature | The SHA-2 digest of the signed part of the file matches the signed digest, the signature is valid for the key of the certificate, and the signing certificate attribute names that certificate | The signed bytes were changed, the ranges do not describe the file, the signature data is damaged or does not match |
| Later changes | Nothing follows the signature, or only signatures, time stamps, validation data and document properties (the information of the document and its XMP packet) were added | The page content, fonts, pictures or structure were changed after signing; for a certifying signature, any change it does not allow. Form fields or comments added later leave the answer open |
| Signer identity | The chain of certificates leads to a root that is in the list of Windows or in your file, and every certificate was valid at the time that matters | A certificate in the chain was not valid then, or is not allowed to issue certificates. A self-signed certificate, an unknown root or a missing issuer leave the answer open |
| Revocation | The answers stored in the file, or the certificate authorities when asked, show no certificate of the chain was revoked | A certificate was revoked before the signature was made. No answer leaves the answer open |
| Time stamp | The stamp is for this signature, signed correctly and issued by a trusted server | The stamp is for other data or its signature is wrong. An untrusted server leaves the answer open |

A signature is **valid and trusted** when none of the checks failed and the first three passed, **not valid** when a check failed, and **valid with open questions** otherwise (for example a self-signed certificate).

The time that matters for the certificates is the time of a valid, trusted time stamp when there is one, and the current time otherwise. A certificate that has expired since the signing, with no time stamp to prove when it was signed, leaves the identity open and says so, instead of failing the signature.

## How it works

1. The signature fields with a value are found in the form of the file, and listed in the order the signatures were added. Document time stamps (`ETSI.RFC3161`) are listed too.
2. The signed ranges are checked against the file: they must start at the beginning, leave out exactly the space of the signature value, and stay inside the file. A range that leaves out more than the signature (to hide changed bytes) is refused.
3. The digest of the signed ranges is compared with the one the signature names, and the signature over the signed attributes is checked with the public key of the signer certificate. RSA (PKCS#1 v1.5 and PSS) and ECDSA (P-256 and P-384) with SHA-1 (flagged as weak), SHA-256, SHA-384 and SHA-512 are read.
4. The chain is built from the certificates in the signature, in the document security store and the trusted ones, and each link is checked by signature, validity, the certificate authority flag and the path length.
5. What follows the signature in the file is sorted by kind of object. Signatures, time stamps, the validation data and the document properties are fine; the page tree, content and fonts are not.
6. Revocation uses the OCSP answers and lists stored in the file, and, with `check_revocation` and `allow_network`, asks the servers named in the certificates (OCSP first, then the list). Each answer is checked: signed by the issuer or a responder it certified, about this certificate, and fresh.

## Behavior and limits

- Windows decides which authorities are trusted. Acrobat uses its own list, so the two programs can disagree about the same signature.
- The check of later changes sorts objects by kind. It does not compare the old and new versions of an object, so a change inside a form field or a page that keeps its kind is counted as form or structure change, and the rules of a certification (DocMDP) are applied only as "none allowed" or "some allowed".
- A password-protected file cannot be checked here (`QW-PDF-ENCRYPTED`): unlock a copy first.
- Signatures with several signers, and algorithms other than the ones listed, are reported as not checkable.
- Missing intermediate certificates are not fetched from the web (`caIssuers`); a chain that needs them is reported with its open end.
- The report is drawn in the standard fonts, so other scripts show as `?`. The checks use the real names.

## Errors

`QW-INPUT-NOT-PDF`, `QW-PDF-ENCRYPTED`, `QW-PDF-DAMAGED`, `QW-PATH-DENIED` (the file of trusted certificates), `QW-OPTION-INVALID` (that file holds no certificate), `QW-NETWORK-CONSENT-REQUIRED`, `QW-OUTPUT-VERIFY-FAILED`, `QW-JOB-CANCELLED`.

## License impact

The same libraries as Digital signature: RustCrypto (`cms`, `x509-cert`, `x509-ocsp`, `rsa`, `p256`, `p384`), `rustls-native-certs` for the root list. No copyleft code is linked.
