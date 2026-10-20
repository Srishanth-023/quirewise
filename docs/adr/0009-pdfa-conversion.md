# ADR-0009: PDF/A by normalizing, checking with veraPDF and falling back

Status: accepted

## Context

PDF/A (and the PDF/UA check) is decided by rules that only a validator can judge. The usual way to
convert is to let Ghostscript rewrite the document, but Ghostscript is AGPL-3.0 and cannot be part
of the core, and it rewrites more than needed. Writing a full PDF/A converter is out of reach, yet
most documents fail the same few rules.

## Decision

- **Validation** is veraPDF, the reference validator, run as a separate Java process: the component
  `verapdf` holds `bin\cli-<version>.jar` and a `jre` folder (ADR-0005). Reports are written by
  Quirewise from veraPDF's JSON, as a page or as data.
- **Conversion** normalizes the document in `lopdf-ops` (fonts embedded from installed fonts, a
  generated sRGB profile, an XMP packet that agrees with the document information, what the
  standard forbids removed), then asks veraPDF. Nothing is saved that veraPDF does not accept for
  the level in the file name.
- **When that fails:** the next weaker level (A to U to B, part 1 to part 2) if the person allows
  it; an installed Ghostscript, in a build with the `agpl-ghostscript` feature, as a separate
  process; pictures of the pages if the person allows it. Otherwise nothing is written and the
  message names the rules that are still broken.
- **Level A** is not produced: tags need an understanding of the document that normalization does
  not have. Tagged input keeps its tags.

## Consequences

- Without veraPDF the tools say so; no unchecked PDF/A file is ever written.
- The result is as good as the rules the normalizer knows. New failures are fixed by adding a rule
  to the normalizer and a fixture to `crates/worker/tests/pdfa.rs`.
- No AGPL code is linked or in the default build.
- Embedding whole installed fonts grows files; subsetting is a later improvement.
