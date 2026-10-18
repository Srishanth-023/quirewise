# ADR-0004: In-house CMS and PAdES signing

Status: accepted

## Context

PAdES signing needs CMS construction and incremental PDF updates. The existing Rust option `pdf_signer` is GPL-3.0, which the core cannot use.

## Decision

Build CMS/PAdES in the `sign-cms` engine on RustCrypto crates. Keys come from PFX files (in memory, zeroized) or the Windows certificate store through CNG, so non-exportable and hardware-token keys never leave CNG. Timestamping and revocation checks need explicit network consent.

## Consequences

- More code to own and test; signing is covered by verification tests in the same engine.
- No GPL dependency in the core.
