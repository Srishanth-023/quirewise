# ADR-0003: PDF engines and license policy

Status: accepted

## Context

The core must stay under permissive licenses and ship on arm64.

## Decision

Use PDFium (render, text, forms, annotations, flatten, attachments) through `pdfium-render`, qpdf (encryption, linearization, repair, page copy, object streams) and `lopdf` (low-level object edits). MuPDF is excluded. Ghostscript (AGPL-3.0) exists only as an opt-in plugin in a separate process behind the Cargo feature `agpl-ghostscript`, off by default, with a license notice. Allowed core licenses: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, MPL-2.0, OFL-1.1, Unicode-3.0, CC0-1.0, enforced by `cargo-deny`.

## Consequences

- Aggressive compression and some PDF/A cases may need the Ghostscript plugin.
- Native binaries are fetched by scripts with pinned SHA-256 and never committed.
