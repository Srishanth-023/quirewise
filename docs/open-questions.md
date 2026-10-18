# Open questions

Questions are recorded here and work continues with the safest default (AGENTS.md section 1).

| # | Question | Default taken | Needs |
|---|---|---|---|
| 1 | SHA-256 pins for PDFium and qpdf in `scripts/native-pins.json` are empty | Fetch scripts refuse to run until a reviewed hash is recorded | One reviewed download per architecture |
| 2 | Exact qpdf arm64 release asset name and PDFium release tag | URLs in `native-pins.json` are provisional | Confirm against the upstream release pages |
| 3 | Code signing: Azure Trusted Signing or a certificate | `scripts/sign.ps1` supports both through environment variables | Owner decision |
| 4 | Git LFS for fixtures over 1 MB | `.gitattributes` routes fixture binaries to LFS | `git lfs install` on dev machines and CI |
| 5 | Windows Job Object CPU-time cap default | 10 minutes per job, configurable | Measure on large fixture in Phase 1 |
| 6 | Windows SDK and ARM64 build tools were missing at the start of Phase 0 | Installed VS 2022 Build Tools by hand; both targets build | None |
| 7 | Job Object sets memory, CPU time and kill-on-close but no active-process limit, because sidecars (LibreOffice, Tesseract) are children of the worker | Each sidecar adapter must start its engine under its own job; revisit in Phase 6 | Design in Phase 6 |
| 8 | Session scratch folder uses the per-user %TEMP% ACL, not an explicit restrictive ACL | Per-user temp is already private; explicit DACL needs unsafe Win32 in the worker | Decide in Phase 13 hardening |
| 9 | One worker process per unit of work | Simple and isolated; pooling comes with performance tuning in Phase 13 | Measure against the 3 s merge budget |
| 10 | Arm64 tests cannot run on the x64 dev machine; the arm64 build and binary check pass | CI arm64 runner or a Windows on Arm machine runs the suite | Runner availability |
| 11 | cargo-deny and cargo-audit are not installed on the dev machine | CI runs them; install locally with cargo install --locked | Owner approval to install |
| 12 | libfuzzer-sys carries an NCSA license part, outside the allowed list | Used only by fuzz targets in a separate workspace, never distributed | ADR if it ever ships |
| 13 | A stray .cargo/config.toml (GNU linker experiment) exists untracked in the repo root | Left untouched and not committed | Owner to delete or keep |
