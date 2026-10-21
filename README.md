# Quirewise

Offline-first PDF toolkit for Windows x64 and arm64. Merge, split, compress, convert, OCR, edit, fill, sign, redact, compare and validate PDFs locally. No uploads, no telemetry. Built with Tauri 2 and Rust on PDFium, qpdf and Tesseract.

## Status

Under construction. Scope and phases are defined in `docs/project/MASTER_PROMPT.md`; engineering rules are in `docs/AGENTS.md`. Progress is tracked in `docs/CHANGELOG.md`.

## Development

Full steps: `docs/SETUP.md`. In short:

Prerequisites: Rust (version pinned in `rust-toolchain.toml`), MSVC build tools with the Windows SDK (and ARM64 build tools for the arm64 target), Node.js 24 and pnpm.

```powershell
pnpm -C apps/desktop/ui install --frozen-lockfile
cargo test --workspace --locked
pnpm -C apps/desktop/ui test
```

The full checklist before a change is merged is in `docs/AGENTS.md` section 1.

## Layout

| Path | Purpose |
|---|---|
| `crates/domain` | Pure types and rules |
| `crates/application` | Use cases and ports |
| `crates/infrastructure` | Adapters: files, settings, logs, components, queue |
| `crates/engines/*` | PDF, OCR, Office, signing and AI engine adapters |
| `crates/worker` | Isolated process that parses untrusted PDFs |
| `crates/cli` | Headless CLI (`qw`) |
| `apps/desktop` | Tauri shell and Svelte UI |
| `crates/test-support` | Shared test helpers and PDF builders |
| `fuzz/` | cargo-fuzz targets |
| `fixtures/` | Test PDFs, images, Office files and golden outputs |
| `tests/` | Cross-crate integration, regression and end-to-end tests |
| `scripts/` | Fetch, stage, sign, SBOM and check scripts (`scripts/README.md`) |
| `vendor/` | Downloaded engines and models (gitignored, filled by `scripts/fetch/fetch-*.ps1`) |
| `docs/` | Guides, design, ADRs, per-tool docs, legal, project notes (`docs/README.md`) |
| `build-msi.ps1` | One-step .msi build |

## License

MIT. See `LICENSE` and `docs/legal/THIRD_PARTY_NOTICES.md`.
