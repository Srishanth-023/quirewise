# Open questions

Questions are recorded here and work continues with the safest default (AGENTS.md section 1).

| # | Question | Default taken | Needs |
|---|---|---|---|
| 1 | SHA-256 pins for PDFium and qpdf | Resolved in Phase 2: pins in `scripts/native-pins.json` come from the digests GitHub publishes per release asset and are re-verified on every fetch | None |
| 2 | qpdf arm64 asset name and PDFium tag | Resolved in Phase 2: msvc-arm64 zip and chromium/8100 confirmed | None |
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
| 14 | The spec token `--border-strong` is 2.2:1 on the page background, below the 3:1 that WCAG 1.4.11 asks for control boundaries | Added an extra token `--control-border` (light #857f74, dark #6f7882, 3.3:1 or better) for control boundaries; spec tokens are unchanged. `contrast.test.ts` enforces AA for all text pairs | Owner to approve the extra token or change the spec value |
| 15 | Installed layout for native files: native/pdfium/pdfium.dll, native/qpdf/qpdf.exe, and quirewise-worker.exe next to the app | The locator and tests assume it; the installer must lay files out this way | Phase 13 NSIS packaging |
| 16 | Inspect scans fonts and images on the first 500 pages only | Report says how many pages were scanned | Revisit with the performance pass |
| 17 | Dev mock for the Tauri IPC (`src/dev/mockTauri.ts`) lets the UI run in a browser; it uses a real PDFium render as the page image | Compiled out of production builds (checked) | None |
| 18 | Split by size packs with measured page sizes (up to 300 pages, the average beyond) and then verifies and halves any file over the limit | Output files can be somewhat under the limit; a single oversize page becomes its own file with a warning | Revisit with the performance pass |
| 19 | Scaling pages (Merge, normalize page size) does not move annotations | Documented in docs/tools/merge.md; flatten forms first for exact results | Handle annotations in the Edit phase |
| 20 | The Merge contents page has no clickable links | Plain list, 35 lines per page | Add link annotations with the Edit tools |
| 21 | Images converted by Merge are sized at 150 dpi, so small images give small pages | Normalize page size is available | Offer a page size choice for images |
| 22 | Per-file options of multi-input tools use one `per_file` array matching the input order; missing entries use defaults | Same convention in Merge and Alternate and Mix; the UI shows one group per file | None |
| 23 | Compress has no "subset fonts" option | No font subsetter is available that fits the license rules; images and structure are optimized instead | Decide on a subsetting approach in a later phase |
| 24 | Remove annotations uses lopdf, not PDFium as the architecture table suggests | lopdf edits `/Annots` and prunes the removed objects exactly; PDFium cannot save such edits without re-rendering | Owner to accept the deviation |
| 25 | Compress touches only page-level images stored as DCT or Flate, 8-bit gray or RGB, and never makes an image bigger | Masks, indexed, CMYK, 16-bit and images inside form XObjects are left alone | Widen coverage with the performance pass |
| 26 | Edit metadata does not prefill the current values, and rewrites the XMP packet (a PDF/A claim is lost) | Blank keeps a field, the Remove switch clears it | Prefill from Inspect in a later UI pass |
| 27 | Sanitize strips hidden layers only from page content streams | Marked content inside form XObjects stays | Walk form XObjects in a later pass |
| 28 | Protect passwords travel to the worker over its private stdio pipe and to qpdf on its standard input; restrictions are advisory | Never on a command line, in logs or on disk | None |
| 29 | Repair needs `%PDF-` near the start of the file, because inputs are recognized by that signature | A file without it is rejected as not a PDF | Add a lenient mode if real files need it |
| 30 | Tools that rewrite a protected file (Compress, Linearize, Remove annotations, Edit metadata, Sanitize, Flatten and the Phase 5 edit tools) could have written an unprotected result | Resolved in Phase 5: the original password and restrictions are copied with qpdf `--copy-encryption`. A file that opens without a password has no owner restrictions to keep | None |
| 31 | Stamps (Page numbers, Header and footer, Bates, Watermark) use the 14 standard PDF fonts and Latin-1 plus Windows-1252 characters only | Other characters are rejected before the run with a clear message; no font is embedded, which keeps files small and licensing simple | Decide on an embedded font (for example Noto) to cover other scripts |
| 32 | Stamping, Crop, Resize, Flip, N-up and Grayscale edit content with lopdf, not PDFium page objects as the architecture table suggests | lopdf writes the exact operators and keeps the rest of the file as it was; PDFium cannot save such edits without re-rendering | Owner to accept the deviation (same reason as row 24) |
| 33 | N-up drops links, form fields, bookmarks and page labels of the source pages | Sheets are new pages made of forms; documented in docs/tools/n-up.md | Carry links over with the Forms phase |
| 34 | Grayscale without the raster fallback leaves shadings, patterns, inline images and CMYK pictures in color | The fallback renders every page in gray; documented in docs/tools/grayscale.md | Widen coverage with the performance pass |
| 35 | Bates numbering runs all files together, in list order, so reordering the list changes the numbers | The numbers follow the list as it is when the run starts | None |
| 36 | Resize and Flip do not move annotations or form fields | Documented; flatten first for exact results (same as row 19) | Handle annotations in a later pass |
| 37 | Bookmarks "auto-generate" uses font size to find headings, so scans and files with uniform text give none | The run fails with a message that suggests OCR or typing the list | Add heading detection from tagged PDF structure |
| 38 | Rename by content writes a renamed copy to the output folder; the input is never renamed | Matches the rule that inputs are never changed | None |
| 39 | The Bookmarks form starts empty and the list replaces the existing bookmarks; it does not prefill them | Documented in docs/tools/bookmarks.md; split by bookmarks and Inspect show the current ones | Prefill from the document in a later UI pass |
| 40 | Extract images reads picture objects with lopdf, not PDFium as the architecture table suggests | lopdf gives the raw stream, so JPEG pictures come out byte for byte; PDFium returns decoded bitmaps | Owner to accept the deviation (same reason as rows 24 and 32) |
| 41 | HTML to PDF prints with headless Microsoft Edge over the DevTools protocol, not a WebView2 control in a hidden window | Same Chromium engine, no unsafe COM code, testable without a window; Edge ships with Windows 11 and current Windows 10 | Owner to accept the deviation; switch to WebView2 if Edge cannot be relied on |
| 42 | Office documents are converted by driving Microsoft Office through PowerShell and COM | No Office code is shipped; a hidden instance is started per run and macros are off. An Office instance can be left running if the worker is killed in the middle of a conversion | Add a watchdog that closes orphaned instances |
| 43 | LibreOffice is a component with no real package yet: the signing key, the component list address and the packages do not exist | A build without a key installs nothing; a LibreOffice already installed on the computer is used; offline `.qwpkg` import is ready | Owner to create the key, host the list and build the packages (docs/components.md) |
| 44 | With LibreOffice, Excel sheet choice, fit to width and page turn only work for `.xlsx` files | The settings are written into a copy of the workbook; other formats get a clear message for sheet choice and keep their own page setup otherwise | Support `.xls` and `.ods` by editing their XML |
| 45 | TIFF to PDF and PDF to TIFF share row 68 as two tools; TIFF frames in floating point or Lab are not supported | The message names the unsupported format | Add them if real files need it |
| 46 | PDF to images writes no resolution tag into PNG files | JPEG and TIFF carry it | Write a pHYs chunk |
| 47 | Layout text in PDF to text does not follow rotated text | Columns are placed from the unrotated positions | Rotate positions by the page rotation |
| 48 | HEIC pictures need the HEIF Image Extensions from the Microsoft Store and PowerShell | The message names the missing extension | Decode HEIC with a bundled library if the licence allows it |
| 49 | A web address typed in HTML to PDF is printed with scripts running and an empty profile, so pages behind a sign-in print their sign-in page | Documented; only the first page load is done | Offer a cookie or token option if it is needed |
