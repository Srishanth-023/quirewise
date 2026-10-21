# docs/project/MASTER_PROMPT.md: Build Quirewise

You are the sole engineer building **Quirewise**, a personal-use, offline-first Windows desktop PDF toolkit for **Windows x64 and Windows arm64**. Build the entire application in one continuous run, phase by phase (section 15), without stopping for confirmation. Read and obey `AGENTS.md` at all times. If this file and `AGENTS.md` conflict, `AGENTS.md` wins on rules and this file wins on scope.

This file is self-contained. **Appendix A (Feature Matrix) is normative**: every row must be implemented with the listed options and defaults. Appendix B lists dependencies and licenses. Appendix C defines shared syntax (page ranges, naming tokens, error codes).

---

## 1. Project overview

- **Goal:** replicate every feature of iLovePDF (web, desktop, mobile, premium) and Sejda (Web and Desktop), plus the recommended extras in Appendix A, processed 100% locally.
- **Users:** one person on their own PCs. No accounts, no cloud, no telemetry, no network by default.
- **Quality bar:** modern, professional, fast, accessible, very lightweight, no feature compromise.

---

## 2. Tech stack (fixed)

| Area | Choice |
|---|---|
| Shell | Tauri 2. Targets `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`. NSIS installer per arch (MSI only after an ADR confirms arm64 support). WebView2 install mode: `downloadBootstrapper` (WebView2 ships with Windows 11 and Windows 10 1803+). |
| Core | Rust stable (pinned in `rust-toolchain.toml`), Cargo workspace |
| UI | Svelte 5 (runes) + TypeScript strict + Vite, static build. Typed IPC generated from Rust (`tauri-specta` or `ts-rs`). No CSS framework with built-in shadows or gradients. |
| Package manager | pnpm (pinned via `packageManager` field) |
| PDF | PDFium (bblanchon/pdfium-binaries, x64 and arm64 DLLs) via `pdfium-render`; qpdf (official Windows arm64 binaries); `lopdf` for low-level object edits |
| OCR | Tesseract 5 + Leptonica (vcpkg `x64-windows` and `arm64-windows`); language data on demand |
| Office to PDF | LibreOffice headless (MPL-2.0, native arm64), on demand. If Microsoft Office is installed, offer it as an alternative engine via COM automation. |
| HTML to PDF | WebView2 `PrintToPdf` in a hidden window. Remote URLs only with explicit per-task network consent. |
| PDF/A, PDF/UA validation | veraPDF (on demand, with bundled arm64/x64 JRE) |
| Local AI | llama.cpp server sidecar (MIT) with GGUF models on demand. Optional detection of a local Ollama endpoint on loopback only, after consent. Off by default. Show each model's license before download. |
| Signatures | CMS/PAdES built on RustCrypto crates; Windows certificate store via CNG for hardware tokens and non-exportable keys |
| Images | `image` crate family; Windows Imaging Component (WIC) for HEIC/HEIF (requires the Windows HEIF extension; show a clear message if missing) |
| AGPL engines | Ghostscript and MuPDF are never linked into the core. Ghostscript may exist only as an opt-in plugin, separate process, with a license notice. |

**Process model:** UI (WebView2) talks to the Tauri core over typed commands. The core dispatches jobs to a separate **worker process** (`quirewise-worker.exe`) that parses untrusted PDFs inside a Windows Job Object (memory, CPU time, child-process limits). Heavy engines (LibreOffice, Tesseract, llama.cpp, veraPDF, optional Ghostscript) run as separate sidecar processes and are never linked in-process.

---

## 3. Scope

### 3.1 In scope
Implement **all 72 rows of Appendix A**. Summary by group (Appendix A holds the options and defaults):

- **Organize (rows 1-13, 69):** merge (PDFs + images), alternate and mix, organize/reorder, split by ranges, every N pages, extract, remove, split by bookmarks, in half, by size, by text, smart split (AI), scan to PDF, insert blank and duplicate pages.
- **Optimize (rows 14-17, 64):** compress, repair, OCR, deskew, linearize.
- **Convert to PDF (rows 18-22, 68):** images (incl. HEIC, multi-page TIFF), Word, PowerPoint, Excel, HTML/URL.
- **Convert from PDF (rows 23-31, 67):** JPG/PNG/TIFF, extract images, Word, PowerPoint, Excel/CSV, text, Markdown, HTML, PDF/A conversion, PDF/A validation.
- **Edit (rows 32-44, 48-50, 65, 66):** rotate, page numbers, header and footer, Bates numbering, watermark, crop, resize, flip, N-up, grayscale, full editor, annotate, remove annotations, metadata, bookmarks, rename by content, attachments, page labels.
- **Forms (rows 45-47):** fill and sign, create forms (with field detection), flatten.
- **Security (rows 51-55, 61-63):** protect, unlock, visual sign, true redaction, compare, PAdES sign, verify signatures, sanitize.
- **Intelligence, opt-in and local only (rows 12, 29, 56, 57):** smart split, Markdown cleanup, summarize, translate, smart redact suggestions.
- **Validation (rows 31, 70):** PDF/A and PDF/UA reports.
- **Cross-cutting (rows 58-60, 71, 72):** Recipes (chained workflows), output naming tokens, batch queue, Inspect/Properties, app shell (preview, recent files, undo, command palette, drag and drop, Explorer integration).

### 3.2 Out of scope (by design, because the app is offline and single-user)
- Cloud pickers and savers (Google Drive, Dropbox, OneDrive web APIs). Local and synced folders work normally through the file system.
- "Request signatures from others" email workflows, signer tracking, audit trails hosted on a server. Replacement: prepare signature fields (row 46) and export a PDF the other person can sign in any reader.
- Accounts, teams, billing, usage limits, server REST API, mobile apps.

---

## 4. Architecture (Clean Architecture)

Dependency direction points inward only: `ui -> app-shell -> application -> domain`. Adapters (`engines/*`, `infrastructure`) implement `application` ports.

- **`domain`**: pure types and rules. `PageRange` parser (Appendix C), `PageSelection`, `ToolId`, per-tool option structs with defaults, `Job`, `JobStatus`, `OutputNamingPattern`, error enums. No I/O. Allowed crates: `serde`, `schemars`, `thiserror`.
- **`application`**: use cases (`RunTool`, `RunRecipe`, `ValidateInput`, `InspectDocument`, `ListComponents`, `InstallComponent`) and ports (traits): `PdfReader`, `PdfWriter`, `Renderer`, `TextExtractor`, `OcrEngine`, `OfficeConverter`, `HtmlConverter`, `Signer`, `PdfaValidator`, `LlmEngine`, `FileStore`, `SettingsStore`, `ComponentStore`, `Clock`, `Logger`.
- **`engines/*`** adapters: `pdfium`, `qpdf`, `lopdf-ops`, `raster`, `ocr-tesseract`, `office-libreoffice`, `office-msoffice`, `html-webview2`, `sign-cms`, `pdfa-verapdf`, `ai-llamacpp`, `gs-plugin` (Cargo feature `agpl-ghostscript`, off by default).
- **`infrastructure`**: atomic file store, settings (TOML), structured logging (`tracing` JSON), component manager (download, verify, install, remove), job queue, worker supervisor, Windows integration (file associations, Explorer menu, jump lists).
- **`worker`**: separate executable that loads untrusted PDFs, executes tool plans, streams progress over stdio IPC (length-prefixed JSON or MessagePack), runs inside a Job Object.
- **`app-shell`** (`src-tauri`): commands, event bridge, window management. Thin; no PDF logic.
- **`ui`**: Svelte. One feature folder per tool. Option forms generated from the Rust JSON Schema.

**Tool contract (Rust):**
```rust
pub trait Tool: Send + Sync {
    fn id(&self) -> ToolId;
    fn options_schema(&self) -> schemars::Schema;
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, ValidationError>;
    fn execute(&self, ctx: &JobContext, plan: Plan) -> Result<Vec<OutputArtifact>, ToolError>;
}
```
Every tool is registered in one `ToolRegistry`. The UI and CLI never hardcode tool lists. `JobContext` carries the cancellation token, progress sink, temp directory, naming service and logger.

---

## 5. Folder structure (create exactly)

```
quirewise/
  AGENTS.md
  CLAUDE.md                      # one line: "Follow AGENTS.md"
  docs/project/MASTER_PROMPT.md
  README.md
  LICENSE
  docs/legal/THIRD_PARTY_NOTICES.md
  docs/legal/PRIVACY.md
  docs/legal/TERMS.md
  CHANGELOG.md
  SECURITY.md
  Cargo.toml                     # workspace
  rust-toolchain.toml
  deny.toml
  .editorconfig
  .gitignore
  .gitattributes
  apps/
    desktop/
      src-tauri/
        Cargo.toml
        tauri.conf.json
        capabilities/
        icons/
        src/{main.rs,commands/,events/,state.rs}
      ui/
        package.json
        vite.config.ts
        tsconfig.json
        src/
          app/{App.svelte,routes/,shell/}
          lib/
            design/{tokens.css,typography.css,reset.css}
            components/{Button,Field,Select,Dialog,Toast,ProgressBar,PageGrid,Thumbnail,CommandPalette,DropZone}/
            features/<tool-id>/{View.svelte,options.ts,README.md}
            ipc/{generated.ts,client.ts}
            i18n/{en.json,index.ts}
            stores/
          tests/
  crates/
    domain/
    application/
    infrastructure/
    worker/
    cli/                         # headless CLI for tests and power users
    engines/
      pdfium/
      qpdf/
      lopdf-ops/
      raster/
      ocr-tesseract/
      office-libreoffice/
      office-msoffice/
      html-webview2/
      sign-cms/
      pdfa-verapdf/
      ai-llamacpp/
      gs-plugin/                 # feature "agpl-ghostscript", off by default
  components/
    manifest.schema.json
    components.json              # signed manifest
    components.json.sig
  tests/
    integration/
    regression/
    e2e/
  fixtures/
    pdf/{valid,encrypted,damaged,forms,scanned,pdfa,signed,large,malicious}/
    office/
    images/
    golden/                      # expected fingerprints + render hashes
  fuzz/
    fuzz_targets/{page_range,naming_pattern,options_json,component_manifest,pdf_load}.rs
  docs/
    architecture.md
    threat-model.md
    performance-budgets.md
    ui-theme.md
    open-questions.md
    adr/0001-tauri-rust.md ...
    tools/<tool-id>.md
  scripts/
    fetch-pdfium.ps1
    fetch-qpdf.ps1
    build-tesseract.ps1
    update-golden.ps1
    check-deps.ps1
    sign.ps1
    sbom.ps1
    verify-arm64.ps1
  .github/
    workflows/{ci.yml,release.yml,fuzz.yml,audit.yml}
    PULL_REQUEST_TEMPLATE.md
    ISSUE_TEMPLATE/
    dependabot.yml
```

---

## 6. .gitignore (use exactly)

```gitignore
# ---------- Rust ----------
/target/
**/target/
**/*.rs.bk
*.pdb

# ---------- Node / frontend ----------
node_modules/
**/node_modules/
apps/desktop/ui/dist/
apps/desktop/ui/.svelte-kit/
apps/desktop/ui/.vite/
*.tsbuildinfo
.eslintcache
.npm/
.pnpm-store/
npm-debug.log*
yarn-error.log*
pnpm-debug.log*

# ---------- Tauri build outputs and installers ----------
apps/desktop/src-tauri/gen/
*.msi
*.msix
*.msixbundle
*.appx
*.exe
*.nsis.zip
*.sig
bundle/
release/
out/

# ---------- Downloaded native binaries, components, models ----------
vendor/
*.dll
*.lib
*.exp
*.traineddata
*.gguf
*.onnx
*.bin
models/
components/cache/

# ---------- Test outputs and coverage ----------
test-output/
tests/**/output/
coverage/
lcov.info
*.profraw
*.profdata
fuzz/artifacts/
fuzz/corpus/
fuzz/coverage/
playwright-report/
test-results/

# ---------- Logs and crash dumps ----------
*.log
logs/
*.dmp
*.mdmp

# ---------- Local config and secrets ----------
.env
.env.*
!.env.example
*.pfx
*.p12
*.pem
*.key
*.snk
secrets/
local.settings.*
*.local.toml
portable.flag
data/

# ---------- OS files ----------
Thumbs.db
ehthumbs.db
Desktop.ini
$RECYCLE.BIN/
.DS_Store

# ---------- IDE / editors ----------
.vscode/*
!.vscode/extensions.json
!.vscode/settings.json
.idea/
*.iml
*.suo
*.user
*.userosscache
*.sln.docstates
.vs/
*.swp
*~

# ---------- Generated reports (published as CI artifacts) ----------
sbom/
reports/

# ---------- Re-include rules (must stay at the end) ----------
# Signed component manifest is committed
!components/components.json.sig
# Test fixtures are committed even if they match binary patterns above
!fixtures/**
# ...except large generated fixtures and actual outputs
fixtures/pdf/large/*.pdf
!fixtures/pdf/large/README.md
fixtures/**/actual/
```

Rules: binary fixtures live only under `fixtures/`. Track files over 1 MB with Git LFS (`.gitattributes`: `fixtures/** filter=lfs diff=lfs merge=lfs -text` for `*.pdf`, `*.png`, `*.tif`, `*.docx`, `*.xlsx`, `*.pptx`). Large fixtures are generated by a script, not committed.

---

## 7. UI theme specification ("Ink and Paper")

Rationale: paper-like neutrals and a single ink-blue accent read as professional document software, avoid purple-black and neon cliches, and keep contrast high.

**Light theme**

| Token | Hex |
|---|---|
| `--bg` | #F4F2EE |
| `--surface` | #FBFAF7 |
| `--surface-2` | #ECE9E3 |
| `--border` | #D6D1C7 |
| `--border-strong` | #A9A398 |
| `--text` | #1F2328 |
| `--text-muted` | #5A5F66 |
| `--accent` | #1F5F8B |
| `--accent-hover` | #184C70 |
| `--on-accent` | #F7F9FB |
| `--focus` | #B4530A |
| `--success` | #2E6B3F |
| `--warning` | #8A5A00 |
| `--danger` | #A3312A |
| `--selection` | #D7E5F0 |

**Dark theme**

| Token | Hex |
|---|---|
| `--bg` | #16191C |
| `--surface` | #1D2125 |
| `--surface-2` | #252A2F |
| `--border` | #363C43 |
| `--border-strong` | #4E565F |
| `--text` | #E6E3DD |
| `--text-muted` | #A3A7AD |
| `--accent` | #6FA8D6 |
| `--accent-hover` | #8DBBE0 |
| `--on-accent` | #10161B |
| `--focus` | #E39A4F |
| `--success` | #7BBF8E |
| `--warning` | #E0B160 |
| `--danger` | #E58A80 |
| `--selection` | #24384A |

- Follow the Windows app theme by default (`prefers-color-scheme`); allow manual override; support Windows contrast themes via `forced-colors: active`.
- **Typography:** IBM Plex Sans (UI), IBM Plex Mono (numbers, ranges, file sizes). Bundle subsetted WOFF2 locally. Scale 12, 13, 14 (base), 16, 20, 24 px. Line height 1.45 body, 1.25 headings. Weights 400, 500, 600.
- **Icons:** Phosphor Icons, Regular weight, 16 and 20 px, `currentColor`. Never Lucide.
- **Spacing scale (px):** 2, 4, 8, 12, 16, 24, 32, 48. 8 px layout grid.
- **Corner radius:** 2 px controls, 4 px panels and dialogs, 0 px page thumbnails. Never larger.
- **Elevation:** 1 px borders and surface color steps only. No `box-shadow` anywhere (lint-enforced).
- **Motion:** 120 to 160 ms ease-out for state changes only; respect `prefers-reduced-motion`.
- **Progress:** determinate progress bars with counts ("Page 14 of 120") and ETA. Spinners only for waits under 500 ms. No skeleton loaders.
- **Previews:** always real rendered thumbnails and before/after previews of the actual file. No placeholder art, no fake demos.
- **Layout:** left navigation (tool groups), central workspace (page grid or preview), right options panel. Collapses to a single column below 1100 px width.

---

## 8. UI avoid-list (hard bans)

Harsh gradients; Lucide icons; pure white (#FFFFFF) backgrounds; rainbow coloring; drop shadows; three feature cards in a row; emojis; liquid glass or frosted blur; em dashes in UI copy or docs; Inter, Geist, Space Grotesk fonts; colored left-stripe accents on cards or alerts; fake testimonials; bento grids; terminal-window visuals; "It's not X, it's Y" copy; check-mark bullet lists; fake or missing product previews; soft large corner radius; purple-and-black theme; skeleton loaders; radial orbs; dot-grid backgrounds; sparkle icons (including for AI features); animated arrows; missing Terms, Privacy, About, License screens; neon colors.

---

## 9. Minimal Touch Points (MTP) rules

- Home screen: one drop zone plus a searchable tool list grouped by category. Dropping files anywhere in the window works on every screen.
- Core flow is at most **3 actions**: drop file, pick tool (or accept the suggested tool), press Run. Smart defaults mean Run works without touching options.
- Suggest tools from input: images suggest "Images to PDF", multiple PDFs suggest "Merge", scanned PDFs (no text layer) suggest "OCR", encrypted PDFs suggest "Unlock".
- Command palette (Ctrl+K) jumps to any tool, recent file or recipe.
- Remember last-used options per tool; "Reset to defaults" is always visible.
- Explorer integration: optional `.pdf` association, "Open with Quirewise", and a Quirewise submenu with the top 5 tools (configurable). Phase 13 ships the classic registry menu (shown under "Show more options" on Windows 11); a top-level Windows 11 menu via `IExplorerCommand` with sparse package identity requires an ADR.
- Batch queue: add many files, one run, per-file status, retry failed only.
- Output is saved next to the source by default with a non-colliding name (`name (compressed).pdf`). Completion panel: Open, Show in folder, Run another tool on result.
- Undo/redo for page-level operations in the organizer; history panel in the editor.
- Keyboard: every action reachable; shortcut sheet on F1.
- Recent files with real thumbnails, clearable, stored locally.
- Password prompt appears inline once per file and is remembered for the session only, in memory.

---

## 10. Backend standards (mandatory)

- **Clean Architecture:** boundaries in section 4, enforced by crate dependencies and `scripts/check-deps.ps1` in CI.
- **SOLID:** one responsibility per module; tools depend on ports, never on concrete engines.
- **Naming:** Rust per API guidelines (snake_case, CamelCase); TS camelCase/PascalCase; tool ids kebab-case (`split-by-size`).
- **Strong typing:** newtypes `PageIndex` (0-based), `PageNumber` (1-based), `Dpi`, `Percent`, `Bytes`, `Millimeters`; no stringly-typed options.
- **DRY:** shared services for page selection, output naming, and stamping (page numbers, header/footer, Bates and watermark share one stamper).
- **Input validation:** magic bytes and structure check before processing; canonicalize paths; reject device paths and reparse-point escapes; UNC paths only if user-picked; validate options against schema and ranges against page count.
- **Error handling:** `thiserror` in libraries; one `AppError` at the boundary with stable codes (Appendix C) and user-facing messages that state what happened and what to do next. No raw stack traces in the UI.
- **Logging:** `tracing` with JSON files in `%LOCALAPPDATA%\Quirewise\logs`, daily rotation, 14-day retention. Never log document content or passwords; file paths hashed at info level.
- **Security and privacy:** untrusted PDFs parsed only in the worker inside a Job Object (default memory cap 2 GB, CPU time cap, no child processes except allowed sidecars); network disabled by default; explicit consent per action for URL-to-PDF, component downloads, TSA timestamps, update checks, loopback AI endpoints; zero telemetry.
- **Performance:** all processing off the UI thread; bounded concurrency (default physical cores minus 1, max 4 heavy jobs); page-at-a-time streaming for large files; cancellation checked per page; RAII disposal of PDFium handles.
- **Data integrity:** never overwrite inputs; write a temp file in the destination directory, flush, verify, then atomic rename (`MoveFileExW`, replace only our own temp outputs). Verification: reopen with PDFium, `qpdf --check`, expected page count, plus tool-specific assertions (encryption opens with the password, PDF/A report passes, redacted text not extractable, signature validates).
- **Configuration:** `%APPDATA%\Quirewise\settings.toml` (preferences), `%LOCALAPPDATA%\Quirewise\` (cache, logs, components, recent-files DB, crashes), `%TEMP%\Quirewise\<session>` (scratch, wiped on exit and on next start). Portable mode: if `portable.flag` sits next to the exe, store everything in `.\data\`. Settings are versioned with migrations.
- **Documentation:** rustdoc on all public items, README per crate, `docs/tools/<tool-id>.md` per tool, ADRs for significant decisions.
- **Code review:** every change via PR with the AGENTS.md checklist; self-review the diff before merge.

---

## 11. Additional standards (added)

- SemVer, Conventional Commits, CHANGELOG (Keep a Changelog) via git-cliff.
- ADRs in `docs/adr`; STRIDE threat model in `docs/design/threat-model.md`, updated for any feature touching files, network or crypto.
- SBOM (CycloneDX) for Rust and npm on every release; `cargo-deny` (licenses, advisories, bans, sources) and `cargo-audit`; npm audit + license checker; Dependabot weekly.
- **Allowed licenses in core:** MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, MPL-2.0, OFL-1.1 (fonts), Unicode-3.0, CC0-1.0. GPL/AGPL only behind opt-in features in separate processes, with notices.
- **Code signing:** Authenticode for exe, dll and installers (Azure Trusted Signing or a code-signing certificate). Component manifest signed with Ed25519 (minisign).
- **Updates:** off by default; optional manual "Check for updates" with the Tauri updater and signature verification.
- **Crash handling:** local-only minidumps and panic reports in `%LOCALAPPDATA%\Quirewise\crashes`; "Open crash folder" in Help; nothing is sent.
- **Accessibility:** WCAG 2.2 AA; full keyboard support; 2 px focus ring in `--focus` with 2 px offset; targets at least 24x24 px; ARIA names exposed to UI Automation; live-region announcements for job progress; per-monitor DPI; 200% zoom without loss.
- **i18n readiness:** all strings in ICU MessageFormat resource files; no string concatenation; RTL-safe layouts; locale-aware numbers and dates.
- **Reproducible builds:** pinned toolchains, `--locked`, pinned native binaries with SHA-256, `SOURCE_DATE_EPOCH`.
- **Performance budgets** (targets; CI fails on regressions over 10%): base installer at most 30 MB per arch; cold start to interactive at most 1.0 s on a mid-range x64 or Snapdragon X laptop; idle RAM at most 150 MB including WebView2; first thumbnail at most 300 ms; merge of 100 small PDFs under 3 s.
- **Legal screens:** LICENSE, docs/legal/THIRD_PARTY_NOTICES.md (generated from audits), docs/legal/TERMS.md, docs/legal/PRIVACY.md (no data collection), About dialog listing versions and licenses of all bundled and installed components and models.

---

## 12. Security model summary

- **Assets:** user documents, passwords, signing keys, local files outside the chosen ones.
- **Threats:** malicious PDFs (parser memory bugs, decompression bombs, JavaScript, launch actions), path traversal via output naming, tampered components, IPC abuse from injected web content, content leaking into logs or temp files.
- **Controls:**
  - Worker isolation with Job Object limits; caps on decompression ratio, object count, page count; per-job timeout.
  - Never execute PDF JavaScript; never follow launch or URI actions.
  - Sanitize output names: strip reserved names (CON, PRN, AUX, NUL, COM1-COM9, LPT1-LPT9), trailing dots and spaces, illegal characters; enforce length limits.
  - Signed components with hash pinning.
  - Tauri capability allow-list per window; no remote content in the WebView.
  - CSP: `default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' asset: http://asset.localhost blob: data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; base-uri 'none'`. Scope the asset protocol to the session temp directory only.
  - Secrets held in memory with `zeroize`; per-session temp directory with restrictive ACLs, deleted on completion.
  - Private keys from PFX (in memory) or the Windows cert store (non-exportable keys stay in CNG).

---

## 13. Testing plan

- **Unit:** every domain type and parser (page ranges, naming tokens, option validation) with property tests (`proptest`).
- **Integration:** every tool through the CLI against fixtures; assert page counts, text presence/absence, metadata, encryption state, validity (`qpdf --check`, PDFium load).
- **Golden files:** per tool, expected fingerprints (page count, per-page text hash, 72 dpi render hash with tolerance) in `fixtures/golden`; regenerate only via `scripts/update-golden.ps1` with reviewed render diffs.
- **Regression:** every fixed bug adds a fixture and a test named `regression_<slug>`.
- **Fuzzing:** `cargo-fuzz` targets for page ranges, naming patterns, options JSON, component manifest, PDF load through the worker; nightly in CI; crashes become fixtures.
- **E2E:** Playwright against the built app for all MTP flows; axe-core checks on every screen.
- **Visual lint:** fail the build on `box-shadow`, `drop-shadow`, `linear-gradient`/`radial-gradient` (unless approved in an ADR), banned fonts, Lucide imports, emoji code points, em dash (U+2014) in UI strings and docs, `border-radius` above 4 px, raw hex colors outside `tokens.css`, `#FFFFFF`/`white`.
- **arm64:** run the full suite natively on Windows arm64 before every release.
- **Coverage:** 85% lines in `domain` and `application`, 70% overall.

---

## 14. CI/CD

- **`ci.yml`** (PR and main): matrix `{x86_64, aarch64}-pc-windows-msvc`; fmt, clippy `-D warnings`, tests, UI typecheck, lint, unit tests, visual lint, `check-deps`, `cargo deny`, build NSIS installers, installer size budget check, upload artifacts. Use a GitHub-hosted Windows arm64 runner if the plan provides one; otherwise cross-compile on x64 and run arm64 tests on a self-hosted Windows on Arm machine.
- **`fuzz.yml`**: nightly, 30 minutes per target.
- **`audit.yml`**: weekly advisories, license report, SBOM.
- **`release.yml`**: on tag `v*`: build both arches, sign, generate SBOM and THIRD_PARTY_NOTICES, create a draft GitHub Release with SHA-256 checksums.

---

## 15. Phased build plan (execute in order, do not skip)

Each phase ends with: all tests green on both arches, docs updated, CHANGELOG entry, Conventional Commit(s). Row numbers refer to Appendix A.

| Phase | Work | Rows |
|---|---|---|
| 0 | Repo scaffold, workspace, tooling, CI, lint and visual-lint rules, design tokens, fonts, icons, AGENTS.md, CLAUDE.md, ADR-0001 to 0005, threat model draft, LICENSE/TERMS/PRIVACY/NOTICES skeletons | - |
| 1 | Domain + ports, ToolRegistry, page range parser, naming service, job queue, batch, worker with Job Object, atomic file store, settings, logging, error codes, CLI | 59, 60 |
| 2 | PDFium + qpdf adapters (both arches), viewer with real thumbnails, drop zone, command palette, recent files, batch queue UI, progress UI, Inspect/Properties | 71, 72 |
| 3 | Organize tools | 1-11, 32, 69 |
| 4 | Optimize and security basics | 14, 15, 44, 47, 48, 51, 52, 63, 64 |
| 5 | Edit tools | 33-41, 49, 50, 65, 66 |
| 6 | Conversions + component manager (signed manifest) + LibreOffice component | 18-24, 28, 68 |
| 7 | OCR, deskew, scan to PDF (Tesseract component, language packs) | 13, 16, 17 |
| 8 | PDF to Office, Markdown, HTML (native layout pipeline, table detection, OCR path) | 25-27, 29, 67 |
| 9 | Editor, annotate, fill and sign, create forms, field detection, visual signatures | 42, 43, 45, 46, 53 |
| 10 | True redaction, compare, PAdES sign and verify | 54, 55, 61, 62 |
| 11 | PDF/A conversion, veraPDF validation, PDF/UA check, optional Ghostscript plugin | 30, 31, 70 |
| 12 | Local AI component: smart split, summarize, translate, smart redact suggestions, Markdown cleanup | 12, 56, 57 (+ AI parts of 29, 54) |
| 13 | Recipes, Explorer integration, portable mode, i18n pass, accessibility audit, performance tuning to budgets, installers, signing, release | 58 |

---

## 16. Definition of done (per feature)

A tool is done only when all of the following hold:
1. Registered in `ToolRegistry` with a JSON Schema for options; defaults match Appendix A.
2. Validation rejects every invalid input with a specific error code and friendly message.
3. Works on x64 and arm64; tested on both.
4. Unit, integration, golden, and negative tests (encrypted, damaged, empty, zero-page, huge, malicious) pass.
5. Output verified before success is reported; inputs untouched (hash before and after).
6. Cancellable; progress per page or per file; memory within budget on the large fixture.
7. UI reachable in at most 3 actions from a drop, keyboard accessible, axe-clean, real preview shown, no banned patterns.
8. Batch mode supported where it makes sense; last settings remembered.
9. Docs: `docs/tools/<tool-id>.md` (purpose, options, limits, engine, license impact); CHANGELOG entry.
10. Any new dependency passes `cargo-deny` and appears in docs/legal/THIRD_PARTY_NOTICES.md.

---

## Appendix A: Feature Matrix (normative)

Legend: **Src** I = iLovePDF, S = Sejda, B = both, X = recommended extra. Defaults in `[brackets]`. **Ph** = build phase.

| # | Feature | Src | Options [defaults] | Engine / approach | Ph |
|---|---|---|---|---|---|
| 1 | Merge PDF (PDFs + images) | B | file order (drag; sort by name or date); per-file page ranges [all]; per-file rotation [0]; per-file password; bookmark per file [on]; table-of-contents page [off]; normalize page size [off]; form fields keep or flatten [keep] | qpdf/lopdf page copy; images to page via `raster`; outline builder | 3 |
| 2 | Alternate & Mix | S | 2+ documents; pages per document per turn [1]; reverse order per document [off] (duplex scans) | domain interleave planner + qpdf page copy | 3 |
| 3 | Organize / visual reorder | B | drag reorder; rotate; delete; duplicate; insert blank; insert pages from file; multi-file with text source labels per page; undo/redo | PDFium thumbnails; op list applied in one write | 3 |
| 4 | Split by ranges | B | ranges (Appendix C) ; merge selected ranges into one file [off] | range parser + qpdf write | 3 |
| 5 | Split every N pages | B | N [1] | same | 3 |
| 6 | Extract pages | B | page selection; output one file [on] or one file per page | same | 3 |
| 7 | Remove / delete pages | B | click thumbnails or ranges | same | 3 |
| 8 | Split by bookmarks | S | outline level [1]; file name from bookmark title [on] | outline read via PDFium/lopdf | 3 |
| 9 | Split in half | S | vertical or horizontal [vertical]; skip first page [off]; skip last page [off]; right-to-left order [off] | duplicate page + CropBox halves | 3 |
| 10 | Split by size | B | max size [10 MB]; a single page larger than the limit becomes its own file with a warning | greedy page packing + size estimate + verify loop | 3 |
| 11 | Split by text | S | user-drawn region; "text starts with" filter [none]; `{text}` naming token | PDFium text in rect per page; new file on text change | 3 |
| 12 | Smart Split (AI) | I | natural-language instruction; preview proposed splits before run | local LLM over per-page text summaries | 12 |
| 13 | Scan to PDF | I | scanner (WIA) or webcam; auto-crop [on]; perspective fix [on]; color / grayscale / B&W [color]; multi-page; optional OCR [off] | Windows WIA via `windows` crate; WebView `getUserMedia`; Leptonica | 7 |
| 14 | Compress | B | preset Extreme / Recommended / Less [Recommended]; advanced: JPEG quality [65]; max image PPI 72/144/288/720 [144]; grayscale [off]; strip metadata [off]; subset fonts [on]; object streams [on]; show before/after size | image downsample and re-encode + qpdf object streams; Ghostscript opt-in plugin | 4 |
| 15 | Repair | B | automatic; report recovered objects and pages | qpdf xref reconstruction; PDFium load + re-save fallback | 4 |
| 16 | OCR | B | languages (Tesseract codes, multi-select) [eng]; output searchable PDF [default] or text; deskew first [off]; page range [all]; skip pages that already have text [on] | Tesseract + Leptonica sidecar; invisible text layer via PDFium | 7 |
| 17 | Deskew | S | automatic [default] or manual per page | Leptonica skew detection; page transform | 7 |
| 18 | Images to PDF | B | formats JPG, PNG, GIF, BMP, TIFF, WebP, HEIC; orientation portrait / landscape / auto [auto]; margin none / small / big [none]; page size fit / A4 / Letter [fit]; one PDF [on] or one per image | `raster` decode; WIC for HEIC | 6 |
| 19 | Word to PDF | B | page range [all]; engine LibreOffice or MS Office [auto] | LibreOffice headless (on demand) or Office COM | 6 |
| 20 | PowerPoint to PDF | I | slides range [all]; include hidden slides [off] | same | 6 |
| 21 | Excel to PDF | I | sheets [all]; fit to page width [on]; orientation [auto] | same | 6 |
| 22 | HTML / URL to PDF | B | source file or URL (URL needs network consent); page size [A4]; orientation [portrait]; margins [10 mm]; background graphics [on]; scale [100%]; header/footer [off]; load timeout [15 s] | WebView2 `PrintToPdf` in hidden window | 6 |
| 23 | PDF to JPG / PNG / TIFF | B | dpi 75 / 150 / 220 / custom [150]; format [JPG]; single multi-page TIFF [off]; color / grayscale / B&W [color]; page selection [all]; naming pattern | PDFium render | 6 |
| 24 | Extract images | B | keep original format [on]; minimum size filter [50x50 px] | PDFium image objects, raw stream export | 6 |
| 25 | PDF to Word | B | layout flowing / exact [flowing]; OCR for scanned pages [auto] | native layout reconstruction to DOCX; LibreOffice optional engine | 8 |
| 26 | PDF to PowerPoint | B | one slide per page; editable text [default] or image slides | native PPTX writer (background + text boxes) | 8 |
| 27 | PDF to Excel / CSV | B | output XLSX / CSV [XLSX]; one sheet per table [default] or per page; detection lattice / stream [auto]; OCR [auto] | native table detector (ruling lines + whitespace); XLSX writer | 8 |
| 28 | PDF to Text | S | plain or layout-preserving [plain]; page separators [on]; encoding UTF-8 [default] | PDFium text | 6 |
| 29 | PDF to Markdown | I | headings, lists, tables, images exported to a folder [on]; optional AI cleanup [off] | layout analysis to Markdown; optional LLM | 8 |
| 30 | PDF to PDF/A | I | level 1a, 1b, 2a, 2b, 2u, 3a, 3b, 3u [2b]; allow downgrade if conversion fails [on]; always validate after conversion | normalization (embed fonts, output intent, XMP) + veraPDF; Ghostscript opt-in | 11 |
| 31 | Validate PDF/A | X | level [auto-detect]; full report export (HTML / JSON) | veraPDF (on demand) | 11 |
| 32 | Rotate | B | 90 / 180 / 270 [90]; pages all / even / odd / selected [all] | `/Rotate` update | 3 |
| 33 | Page numbers | B | pages [all]; start number [1]; first page is cover (skip) [off]; facing pages (mirror position) [off]; position 9-grid [bottom center] + x/y offsets; font, size [10 pt], color; template [`{page}`] e.g. `Page {page} of {pages}`; style arabic / roman / letters [arabic] | shared stamper via PDFium page objects | 5 |
| 34 | Header & Footer | S | header and footer text with tokens (`{page}`, `{pages}`, `{date}`, `{filename}`); font, size, alignment; pages [all] | shared stamper | 5 |
| 35 | Bates numbering | S | prefix; suffix; digits [6]; start [1]; continue across multiple files [on]; position [bottom right] | shared stamper with cross-file counter | 5 |
| 36 | Watermark | B | text or image; pages [all]; position 9-grid [center]; mosaic (tile) [off]; rotation 0-360 [0]; transparency 1-100 [50]; font, style, size, color; layer above or below content [above] | PDFium page objects, ExtGState alpha | 5 |
| 37 | Crop | B | visual box or numeric margins; apply to all / even / odd / selected [all]; auto-trim whitespace [off] | set CropBox (MediaBox optional) | 5 |
| 38 | Resize / page size | S | target size (A4, Letter, Legal, A3, custom) [A4]; margins / padding [0]; scale content to fit [on] | Form XObject wrap + transform | 5 |
| 39 | Flip | S | horizontal or vertical [horizontal]; pages [all] | content matrix transform | 5 |
| 40 | N-up | S | pages per sheet 2 / 4 / 8 / 16 / 32 [2]; order horizontal / vertical [horizontal]; preserve page size [on]; right-to-left [off] | Form XObjects on new pages | 5 |
| 41 | Grayscale | S | text and images [both] | image re-encode + color operator rewrite; raster fallback | 5 |
| 42 | Edit PDF | B | add text, images, SVG, shapes, links; edit existing text; whiteout; use locally installed fonts | PDFium page objects + annotations | 9 |
| 43 | Annotate | S | highlight, strikeout, underline, sticky notes, freehand; color | PDFium annotations | 9 |
| 44 | Remove annotations | S | by type (highlights, strikeouts, notes, links, all) [all] | PDFium annotation removal | 4 |
| 45 | Fill & Sign | B | fill AcroForm fields; drawn / typed / image signature; initials; date stamp | PDFium form-fill environment | 9 |
| 46 | Create forms | B | text, checkbox, radio, dropdown, signature field; auto-detect fields [suggest, user confirms] | AcroForm writer; line/box detection heuristics | 9 |
| 47 | Flatten | S | form fields and annotations only [default]; or everything to image at 75 / 100 / 150 / 220 dpi [150] | PDFium `FPDFPage_Flatten`; raster mode | 4 |
| 48 | Edit metadata | S | Title, Author, Subject, Keywords, Creator, Producer, created/modified dates, Trapped; remove all [off] | Info dictionary + XMP kept in sync | 4 |
| 49 | Bookmarks editor | S | add, nest, rename, reorder, link to page; auto-generate from headings [off] | outline writer | 5 |
| 50 | Rename by content | S | user-drawn region; pattern with tokens (Appendix C) | text in rect + filename sanitizer | 5 |
| 51 | Protect | B | user (open) password; owner password; AES-256 [default]; permissions: print, copy, modify, annotate, fill forms, assemble [all allowed] | qpdf encryption | 4 |
| 52 | Unlock | B | password input; remove restrictions | qpdf decrypt (user password required if set) | 4 |
| 53 | Sign PDF (visual) | B | drawn / typed / uploaded signature; initials; company stamp; date; saved signatures stored locally and encrypted with DPAPI | image or vector stamp | 9 |
| 54 | Redact | I | draw areas; search terms; regex presets (email, phone, IBAN, card number, custom); pages [all]; fill color [black]; AI suggestions [off]; strip metadata [on] | true removal of text, image and vector content under areas; verify by re-extraction; raster fallback for unsupported content | 10 |
| 55 | Compare | I | text diff side by side; visual overlay with tolerance [2%]; export diff report | PDFium text + diff; pixel diff of renders | 10 |
| 56 | AI Summarizer | I | output language [UI language]; length short / medium / long [medium]; output PDF or Markdown [PDF] | local llama.cpp (opt-in) | 12 |
| 57 | Translate PDF | I | target language; preserve layout [on] | local LLM + reflow into text objects | 12 |
| 58 | Recipes (workflows) | S | chain any tools; save, rename, export/import as JSON; run on files or a folder | domain pipeline of tool steps | 13 |
| 59 | Output naming | B | pattern tokens (Appendix C); default `{filename} ({tool})`; collision handling adds ` (2)`, ` (3)` | shared naming service | 1 |
| 60 | Batch processing | B | multiple files per job; per-file status (done, wrong password, damaged, out of range, cancelled); retry failed only | job queue | 1 |
| 61 | Digital signature (PAdES) | X | PFX/P12 or Windows cert store; visible or invisible; reason, location, contact; timestamp via TSA (network consent) [off]; level B-B / B-T / B-LT / B-LTA [B-B]; certify with DocMDP [off] | CMS on RustCrypto; incremental update | 10 |
| 62 | Verify signatures | X | integrity, certificate chain, revocation (network consent), timestamp; report | `sign-cms` module | 10 |
| 63 | Sanitize | X | remove metadata, XMP, JavaScript, embedded files, hidden layers, form actions, thumbnails, private app data [all on] | lopdf/qpdf object walk | 4 |
| 64 | Linearize (fast web view) | X | on / off | `qpdf --linearize` | 4 |
| 65 | Attachments | X | list, add, extract, delete | PDFium attachment API | 5 |
| 66 | Page labels | X | ranges with roman / arabic / letters and prefix | `/PageLabels` writer | 5 |
| 67 | PDF to HTML | X | single file with inline images [on] | layout analysis to HTML | 8 |
| 68 | Multi-page TIFF to PDF and back | X | compression (LZW, Deflate, CCITT G4 for B&W) [LZW] | `raster` + WIC | 6 |
| 69 | Insert blank / duplicate pages | X | position; page size [same as neighbor]; count [1] | qpdf | 3 |
| 70 | Accessibility check (PDF/UA) | X | report export | veraPDF PDF/UA profile | 11 |
| 71 | Inspect / Properties | X | PDF version, page count and sizes, fonts (embedded or not), images (count, dpi), encryption and permissions, signatures, forms, JavaScript presence, PDF/A claim, file size breakdown | PDFium + lopdf | 2 |
| 72 | App shell | X | real preview, recent files, undo, command palette, drag and drop anywhere, Explorer integration, settings, legal screens | UI shell + infrastructure | 2 |

---

## Appendix B: Dependencies and licenses

"Audit" means widely documented but must be confirmed by `cargo-deny` / license check in CI before release.

| Component | Purpose | License | Windows arm64 | Delivery |
|---|---|---|---|---|
| Tauri 2 | Shell, IPC, bundler | MIT / Apache-2.0 (audit) | Native ARM64 app; NSIS installer runs emulated | Bundled |
| WebView2 | UI runtime, HTML to PDF | Part of Windows | Yes | System (bootstrapper) |
| PDFium (bblanchon/pdfium-binaries) | Render, text, forms, annotations, flatten, attachments | BSD-3 / Apache-2.0 (audit) | Yes (`pdfium-win-arm64`) | Bundled |
| pdfium-render | Rust bindings | MIT OR Apache-2.0 | Yes | Bundled |
| qpdf | Encrypt/decrypt, linearize, repair, object streams, page copy | Apache-2.0 | Yes (official since 12.4.2) | Bundled |
| lopdf | Low-level object edits | MIT (audit) | Pure Rust | Bundled |
| Tesseract + Leptonica | OCR, deskew | Apache-2.0 / BSD-2-style | Yes (vcpkg `arm64-windows`) | Engine bundled or on demand; languages on demand |
| LibreOffice headless | Office to PDF, optional PDF to Office | MPL-2.0 | Yes (native since 24.8) | On demand |
| veraPDF + JRE | PDF/A and PDF/UA validation | GPLv3+ or MPLv2+ (use under MPL) | Needs arm64 JRE (ADR) | On demand |
| llama.cpp | Local AI | MIT | Yes | On demand, opt-in |
| GGUF models | Local AI | Per model (shown before download) | n/a | On demand, opt-in |
| Ghostscript | Opt-in aggressive compression, PDF/A fallback | AGPL-3.0 or commercial | Verify per release | Opt-in plugin, separate process |
| MuPDF | Not used | AGPL-3.0 or commercial | n/a | Excluded |
| pdf_signer (Rust) | PAdES alternative | GPL-3.0 (verify) | Pure Rust | Not used by default; prefer in-house CMS (ADR) |
| IBM Plex Sans / Mono | Typography | OFL-1.1 (audit) | n/a | Bundled |
| Phosphor Icons | Icons | MIT (audit) | n/a | Bundled (tree-shaken SVG) |

**On-demand component strategy:** signed `components.json` lists id, version, arch, URL, size, SHA-256 and Ed25519 signature. Download only after explicit consent; verify hash and signature before extraction; install to `%LOCALAPPDATA%\Quirewise\components\<id>\<version>\`; removable from Settings; offline import from a local `.qwpkg` file.

**Known quality limits (show in UI):** PDF to Word/PowerPoint/Excel fidelity depends on source layout; PDF/A conversion may need downgrade or the Ghostscript plugin for complex files.

---

## Appendix C: Shared syntax and codes

### C.1 Page range grammar (1-based, whitespace ignored)
- `5` single page; `3-8` range; `10-` page 10 to end; `-3` pages 1 to 3; `last`; `last-2` (third from last); `even`; `odd`; `all`.
- Comma-separated: `1,3,5-8,last`.
- Reject: zero, negative, start greater than end, pages beyond the count (error `QW-RANGE-OUT-OF-BOUNDS` with the valid range in the message).
- Duplicates allowed only in merge, organize and alternate-mix.

### C.2 Output naming tokens
`{filename}` source name without extension; `{tool}` tool label; `{date}` (YYYY-MM-DD); `{time}` (HHmmss); `{n}` sequence; `{n:000}` zero-padded sequence; `{page}` current page; `{pages}` total pages; `{bookmark}` bookmark title; `{text}` text read from a region. Sejda-style aliases accepted: `[TEXT]`, `[CURRENTPAGE]`, `[BASENAME]`, `[TIMESTAMP]`. Results are always sanitized (section 12).

### C.3 Error code catalog (starter set, extend as needed)
| Code | Meaning |
|---|---|
| `QW-INPUT-NOT-PDF` | File is not a PDF |
| `QW-PDF-ENCRYPTED` | Password required |
| `QW-PDF-WRONG-PASSWORD` | Password incorrect |
| `QW-PDF-DAMAGED` | File is damaged; suggest Repair |
| `QW-PDF-LIMIT-EXCEEDED` | Safety limit hit (size, objects, pages, memory) |
| `QW-RANGE-OUT-OF-BOUNDS` | Page range outside document |
| `QW-RANGE-SYNTAX` | Page range syntax error |
| `QW-OPTION-INVALID` | Option value invalid |
| `QW-PATH-DENIED` | Path not allowed |
| `QW-OUTPUT-VERIFY-FAILED` | Output failed verification; input untouched |
| `QW-COMPONENT-MISSING` | Required component not installed; offer install |
| `QW-COMPONENT-VERIFY-FAILED` | Component hash or signature mismatch |
| `QW-ENGINE-TIMEOUT` | Engine exceeded time limit |
| `QW-JOB-CANCELLED` | Cancelled by user |
| `QW-NETWORK-CONSENT-REQUIRED` | Action needs network consent |
