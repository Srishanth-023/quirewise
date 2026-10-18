# Quirewise

Offline-first PDF toolkit for Windows x64 and arm64. Merge, split, compress, convert, OCR, edit, fill, sign, redact, compare and validate PDFs locally. No uploads, no telemetry. Built with Tauri 2 and Rust on PDFium, qpdf and Tesseract.

## Status

Under construction. Scope and phases are defined in `MASTER_PROMPT.md`; engineering rules are in `AGENTS.md`. Progress is tracked in `CHANGELOG.md`.

## Development

Prerequisites: Rust (version pinned in `rust-toolchain.toml`), MSVC build tools with the Windows SDK (and ARM64 build tools for the arm64 target), Node.js 24 and pnpm.

```powershell
pnpm -C apps/desktop/ui install --frozen-lockfile
cargo test --workspace --locked
pnpm -C apps/desktop/ui test
```

The full checklist before a change is merged is in `AGENTS.md` section 1.

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
| `docs/` | Architecture, threat model, ADRs, per-tool docs |

## License

MIT. See `LICENSE` and `THIRD_PARTY_NOTICES.md`.
