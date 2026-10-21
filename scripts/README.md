# Scripts

Run them from any folder. Each script finds the repository root itself.

| Folder | Scripts |
|---|---|
| `fetch/` | `fetch-pdfium.ps1`, `fetch-qpdf.ps1`, `fetch-tessdata.ps1`, `fetch-verapdf.ps1`, `fetch-llama.ps1`, `build-tesseract.ps1`: pinned engines into `vendor/` |
| `package/` | `stage-installer.ps1`, `make-portable.ps1` (and `build-msi.ps1` in the root) |
| `release/` | `sign.ps1`, `sbom.ps1` |
| `checks/` | `check-deps.ps1` (layer rules), `verify-arm64.ps1`, `update-golden.ps1` |
| `lib/`, `native-pins.json` | Shared fetch code and the SHA-256 pins |
