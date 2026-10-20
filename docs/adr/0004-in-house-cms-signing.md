# ADR-0004: In-house CMS and PAdES signing

Status: accepted

## Context

PAdES signing needs CMS construction and incremental PDF updates. The existing Rust option `pdf_signer` is GPL-3.0, which the core cannot use.

## Decision

Build CMS/PAdES in the `sign-cms` engine on RustCrypto crates. Keys come from PFX files (in memory, zeroized) or the Windows certificate store through CNG, so non-exportable and hardware-token keys never leave CNG. Timestamping and revocation checks need explicit network consent.

## Consequences

- More code to own and test; signing is covered by verification tests in the same engine.
- No GPL dependency in the core.

## Update (phase 10)

Built as decided, on these crates: `cms`, `x509-cert`, `x509-ocsp`, `der`, `spki`, `rsa`, `p256`, `p384`, `sha2` (all RustCrypto) and `p12-keystore` for PFX files. A signature is `ETSI.CAdES.detached` with the content type, message digest and signing certificate (ESS) as signed attributes, appended to the file as an incremental update so earlier bytes and signatures stay valid. The Windows store signs through CNG (`NCryptSignHash`) in one module that allows unsafe code, each call with a `SAFETY` note.

- **Network.** Time stamps (RFC 3161), OCSP answers and revocation lists are fetched only when the person turned on the internet switch for that run. The web client allows `http` because these services are published that way and every answer is signed and checked. Only hashes and certificate numbers are sent; the event kind is logged without an address.
- **Levels.** B-B offline; B-T adds a time stamp; B-LT adds a document security store with the certificates and answers; B-LTA adds a time stamp over the whole file.
- **Trust.** The root list of Windows plus a file the person gives, never the Adobe list.
- **Advisory.** `rsa` has RUSTSEC-2023-0071 (timing, no fix). It is ignored in `deny.toml` because Quirewise never decrypts for a remote party.
- **Checked against other implementations.** The CMS structures made here verify with OpenSSL (`openssl cms -verify`) for RSA and P-256 keys. Files signed by this engine validate as intact, trusted and covering the whole file in pyHanko 0.38 (RSA 2048 and P-256, with PFX files made by OpenSSL 3.5), and files signed by pyHanko, including one added on top of a file signed here, are read correctly. Writing a second signature with pyHanko also added an XMP packet, which is why document properties count as harmless later changes. The levels B-T, B-LT (document security store with an OCSP answer) and B-LTA (document time stamp) validate as trusted in pyHanko too, with the stamp server and the revocation answer taken from the file. Its validator requires the time stamping purpose on the certificate of the stamp server (RFC 3161), so this engine checks it as well.
