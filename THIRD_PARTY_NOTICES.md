# Third-party notices

This file is generated from dependency audits (`cargo deny`, `pnpm licenses`) before each release. The tables below list the components decided so far. Versions and full license texts are added by the release pipeline.

## Bundled design assets

| Component | License | Notes |
|---|---|---|
| IBM Plex Sans, IBM Plex Mono | OFL-1.1 | Subsetted WOFF2, license text in `apps/desktop/ui/src/lib/design/fonts/OFL-1.1.txt` |
| Phosphor Icons (Regular) | MIT | Tree-shaken SVG |

## Planned bundled and on-demand components

| Component | License | Delivery |
|---|---|---|
| Tauri 2 | MIT or Apache-2.0 | Bundled |
| PDFium (bblanchon/pdfium-binaries) | BSD-3-Clause and Apache-2.0 | Bundled |
| pdfium-render | MIT or Apache-2.0 | Bundled |
| qpdf | Apache-2.0 | Bundled |
| lopdf | MIT | Bundled |
| Tesseract and Leptonica | Apache-2.0 and BSD-2-Clause style | Bundled or on demand |
| LibreOffice headless | MPL-2.0 | On demand |
| veraPDF | MPL-2.0 (used under the MPL option) | On demand |
| llama.cpp | MIT | On demand, opt-in |
| Ghostscript | AGPL-3.0 | Opt-in plugin, separate process |

Ghostscript is never linked into Quirewise. MuPDF is not used.

## Rust crates used directly (Phase 1)

| Crate | License | Use |
|---|---|---|
| serde, serde_json | MIT or Apache-2.0 | Serialization |
| schemars | MIT | Options JSON Schema |
| thiserror | MIT or Apache-2.0 | Error types |
| zeroize | MIT or Apache-2.0 | Clearing passwords from memory |
| toml | MIT or Apache-2.0 | settings.toml |
| sha2 | MIT or Apache-2.0 | Path hashing in logs |
| time | MIT or Apache-2.0 | Local date and time |
| tracing, tracing-subscriber, tracing-appender | MIT | JSON logging with rotation |
| clap | MIT or Apache-2.0 | qw CLI |
| windows | MIT or Apache-2.0 | Job Object confinement |
| tauri, tauri-build | MIT or Apache-2.0 | Shell |
| proptest, tempfile | MIT or Apache-2.0 | Tests only |
| libfuzzer-sys | MIT or Apache-2.0, plus NCSA | Fuzz harness only, built in its own workspace, never shipped (see docs/open-questions.md) |

## Added in Phase 2

| Component | License | Use |
|---|---|---|
| PDFium (bblanchon/pdfium-binaries chromium/8100) | BSD-3-Clause and Apache-2.0 | Rendering, text, document facts. Fetched by script, not committed |
| qpdf 12.4.2 | Apache-2.0 | Structural check and page copy sidecar. Fetched by script, not committed |
| pdfium-render | MIT or Apache-2.0 | Rust bindings for PDFium |
| lopdf | MIT | Raw structure facts |
| image (JPEG, PNG, GIF, BMP, TIFF, WebP decoders) and its decoders gif, tiff, image-webp, weezl, half, fax, color_quant, quick-error, crunchy, zerocopy | MIT or Apache-2.0 (zerocopy also BSD-2-Clause) | PNG encoding of previews; image to PDF page conversion in Merge |
| flate2 | MIT or Apache-2.0 | Deflate for converted image streams |
| base64 | MIT or Apache-2.0 | Binary data in IPC messages |
| tauri-plugin-dialog, tauri-plugin-opener | MIT or Apache-2.0 | File picker, open result |
| intl-messageformat | BSD-3-Clause | ICU message formatting in the UI |
| @tauri-apps/api | MIT or Apache-2.0 | Tauri bridge in the UI |
| axe-core, json-schema-to-typescript | MPL-2.0, MIT | Development only: accessibility tests, type generation |
