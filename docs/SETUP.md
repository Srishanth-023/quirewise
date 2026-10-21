# Setup

Build and run Quirewise on Windows 10 or 11 (x64 or arm64).

## Quick start

```powershell
git clone <repository-url> quirewise; cd quirewise
pnpm -C apps/desktop/ui install --frozen-lockfile
scripts/fetch/fetch-pdfium.ps1 -Arch x64
scripts/fetch/fetch-qpdf.ps1 -Arch x64
cd apps/desktop; .\ui\node_modules\.bin\tauri.cmd dev
```

Use `-Arch arm64` on an arm64 machine. The first run compiles the whole workspace and takes a few minutes.

## Requirements

| Need | Details |
|---|---|
| Rust | Through `rustup`. `rust-toolchain.toml` pins the version and both Windows targets. |
| MSVC build tools | "Desktop development with C++" and the Windows SDK. Add the ARM64 tools for arm64. |
| Node.js 24 and pnpm | The pnpm version is pinned in `apps/desktop/ui/package.json`. |
| WebView2 | Included in Windows 11. |
| `cargo-deny` | Optional, for the dependency check. |

## Engines

`vendor/` holds downloaded engines and is not committed. Each download is checked against a SHA-256 pin in `scripts/native-pins.json`.

| Needed for | Script |
|---|---|
| Everything (rendering, page operations) | `fetch/fetch-pdfium.ps1`, `fetch/fetch-qpdf.ps1` |
| OCR | `fetch/build-tesseract.ps1`, `fetch/fetch-tessdata.ps1` |
| PDF/A validation (needs Java) | `fetch/fetch-verapdf.ps1` |
| Local AI | `fetch/fetch-llama.ps1 -Model smollm2-360m-instruct` |

Tests for the optional engines skip themselves when the files are missing. See `docs/guides/components.md`.

## Run

| What | Command |
|---|---|
| Desktop app | `cd apps/desktop` then `.\ui\node_modules\.bin\tauri.cmd dev` |
| UI only, in a browser | `pnpm -C apps/desktop/ui dev`, then open http://localhost:5173 |
| Command line (`qw`) | `cargo run -p quirewise-cli -- tools list` |

The Tauri CLI only finds the project when started from `apps/desktop`. If the app reports a missing worker, run `cargo build -p quirewise-worker`.

## Check

Run these before every commit, one cargo command at a time.

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/checks/check-deps.ps1
pnpm -C apps/desktop/ui check
pnpm -C apps/desktop/ui lint
pnpm -C apps/desktop/ui test
```

Speed budgets: `cargo test --release -p quirewise-worker --test perf`. The full checklist is in `docs/AGENTS.md`.

## Package

| Output | Command |
|---|---|
| MSI | `.\build-msi.ps1` (fast build, result in `dist\`); add `-Full` for the fully optimized release build |
| NSIS installer | steps in `docs/guides/installation.md` |
| Portable zip | `scripts/package/make-portable.ps1 -Target x86_64-pc-windows-msvc` |

Signing and release need the owner's certificate and keys (`docs/guides/installation.md`).

## Troubleshooting

| Symptom | Fix |
|---|---|
| "run scripts/fetch/fetch-pdfium.ps1 first" | Fetch the engines (see above). |
| `tauri` cannot find the project | Start it from `apps/desktop`. |
| "blocking waiting for file lock" | Another cargo command is running. Wait for it. |
| The MSI build times out | Tauri downloads WiX from GitHub. Retry on a working connection. |

Folder layout: `README.md`. All documentation: `docs/README.md`.
