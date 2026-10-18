# Open questions

Questions are recorded here and work continues with the safest default (AGENTS.md section 1).

| # | Question | Default taken | Needs |
|---|---|---|---|
| 1 | SHA-256 pins for PDFium and qpdf in `scripts/native-pins.json` are empty | Fetch scripts refuse to run until a reviewed hash is recorded | One reviewed download per architecture |
| 2 | Exact qpdf arm64 release asset name and PDFium release tag | URLs in `native-pins.json` are provisional | Confirm against the upstream release pages |
| 3 | Code signing: Azure Trusted Signing or a certificate | `scripts/sign.ps1` supports both through environment variables | Owner decision |
| 4 | Git LFS for fixtures over 1 MB | `.gitattributes` routes fixture binaries to LFS | `git lfs install` on dev machines and CI |
| 5 | Windows Job Object CPU-time cap default | 10 minutes per job, configurable | Measure on large fixture in Phase 1 |
| 6 | Local development machine lacks Windows SDK and ARM64 build tools at the start of Phase 0 | Rust work waits for the toolchain; UI work proceeds | Install "Desktop development with C++" with ARM64 tools |
