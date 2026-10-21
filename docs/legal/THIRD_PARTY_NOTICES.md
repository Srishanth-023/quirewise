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
| libfuzzer-sys | MIT or Apache-2.0, plus NCSA | Fuzz harness only, built in its own workspace, never shipped (see docs/project/open-questions.md) |

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

## Added in Phase 6

| Component | License | Use |
|---|---|---|
| ureq, ureq-proto, native-tls, schannel | MIT or Apache-2.0 (schannel MIT) | HTTPS downloads of components, through the Windows TLS stack |
| minisign-verify | MIT | Signature check of the component list and offline packages |
| minisign (tests only) | MIT | Creates test keys and signatures |
| zip and its compression helpers (zopfli) | MIT, zopfli Apache-2.0 | Unpacking components, reading and writing Office packages |
| tungstenite | MIT or Apache-2.0 | Local WebSocket to headless Edge for HTML to PDF |
| tiff, fax, weezl | MIT; weezl MIT or Apache-2.0 | Reading TIFF files, CCITT Group 4 and LZW coding |
| base64 | MIT or Apache-2.0 | Decoding the PDF the browser prints |
| LibreOffice (downloaded on demand, never shipped in the installer) | MPL-2.0 | Word, PowerPoint and Excel to PDF when Microsoft Office is not installed |
| Microsoft Edge, Microsoft Office (the person's own copies, not shipped) | their own licences | HTML to PDF, Office to PDF |

## Added in Phase 7

| Component | License | Use |
|---|---|---|
| Tesseract 5.5 (downloaded on demand, never shipped in the installer) | Apache-2.0 | OCR |
| Leptonica, libpng, libjpeg-turbo, libtiff, zlib, libwebp, OpenJPEG, giflib, libarchive, libcurl and OpenSSL (the libraries Tesseract is built with, shipped inside the Tesseract component) | BSD-2-Clause, libpng-2.0, IJG and BSD-3-Clause, libtiff, zlib, BSD-3-Clause, BSD-2-Clause, giflib, BSD-2-Clause, curl, Apache-2.0 | OCR |
| tessdata_fast language data (downloaded on demand) | Apache-2.0 | OCR languages |

## Added in Phase 8

| Component | License | Use |
|---|---|---|
| roxmltree (development and tests only, not shipped) | MIT or Apache-2.0 | Checks that written Open XML parts are well formed |

The Word, PowerPoint and Excel files are written with the `zip` and `image` crates that earlier phases already list.

## Added in Phase 9

| Component | License | Use |
|---|---|---|
| ttf-parser 0.25 | MIT or Apache-2.0 | Reads installed TrueType fonts so they can be embedded in edited PDFs |
| windows 0.62 (Windows only) | MIT or Apache-2.0 | DPAPI sealing of saved signatures |
| base64 | MIT or Apache-2.0 | Decodes signature pictures sent as data |

## Added in Phase 10

| Component | License | Use |
|---|---|---|
| regex | MIT or Apache-2.0 | Searches the text of pages for Redact (linear time, so a pattern cannot hang a run) |
| similar | Apache-2.0 | Line differences for Compare |
| cms, x509-cert, x509-ocsp, der, spki | MIT or Apache-2.0 | Builds and reads signatures (CMS), certificates and OCSP answers |
| rsa, p256, p384, ecdsa, signature | MIT or Apache-2.0 | RSA and ECDSA signing and checking |
| sha1, sha2 | MIT or Apache-2.0 | Hashes for signatures (SHA-1 only to read old ones) |
| p12-keystore | MIT or Apache-2.0 | Reads PFX and P12 certificate files (and, for tests, writes them) |
| rustls-native-certs | Apache-2.0, ISC or MIT | Reads the trusted root certificates of Windows |
| rand_core | MIT or Apache-2.0 | Random numbers for signing and request numbers |

## Added in Phase 11

| Component | License | Use |
|---|---|---|
| veraPDF 1.30 (greenfield command line) | GPLv3 or MPLv2, used under MPL-2.0 | Checks PDF/A and PDF/UA. Runs as a separate Java process and is an on-demand component; nothing is linked |
| A Java runtime (bundled with the `verapdf` component) | The license of the runtime that the owner picks (for example Eclipse Temurin: GPLv2 with Classpath Exception) | Runs veraPDF; the owner records the exact runtime when building the component |
| Ghostscript (optional, not shipped) | AGPL-3.0 or commercial | Only run as a separate process by a build with the `agpl-ghostscript` feature, from a copy the person installed. Never linked, never in the default build |

## Added in Phase 12

| Component | License | Use |
|---|---|---|
| llama.cpp server (b11755, CPU builds) | MIT | Runs the language models. A separate process and an on-demand component; nothing is linked. The release also holds LLVM OpenMP (Apache-2.0 with LLVM exceptions) |
| Qwen2.5 1.5B Instruct and 7B Instruct (GGUF) | Apache-2.0 | Optional model files, downloaded on demand after the license is shown. Not part of the installer |
| SmolLM2 360M Instruct (GGUF) | Apache-2.0 | Optional small model file, downloaded on demand. Not part of the installer |
| getrandom | MIT or Apache-2.0 | The random key that protects the local server |
| Ollama (not shipped) | MIT | Optional: a program the person already runs. Only used at an address on this computer, after they agree |
