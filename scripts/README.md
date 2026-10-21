# Scripts

| Group | Scripts |
|---|---|
| Fetch pinned engines into `vendor/` | `fetch-pdfium.ps1`, `fetch-qpdf.ps1`, `fetch-tessdata.ps1`, `fetch-verapdf.ps1`, `fetch-llama.ps1`, `build-tesseract.ps1`; pins in `native-pins.json`, shared code in `lib/` |
| Package | `stage-installer.ps1`, `make-portable.ps1` (and `build-msi.ps1` in the root) |
| Release | `sign.ps1`, `sbom.ps1` |
| Checks | `check-deps.ps1` (layer rules), `verify-arm64.ps1`, `update-golden.ps1` |
