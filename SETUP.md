# Set up and run Quirewise

Windows 10 or 11, x64 or arm64.

## 1. Prerequisites

| Tool | Notes |
|---|---|
| Git | |
| Rust | `rustup`; the pinned toolchain in `rust-toolchain.toml` installs itself, with both Windows targets |
| MSVC build tools | "Desktop development with C++" and the Windows SDK; add the ARM64 build tools for the arm64 target |
| Node.js 24 and pnpm | pnpm version is pinned in `apps/desktop/ui/package.json` |
| WebView2 | Part of Windows 11; the installer fetches it on Windows 10 |
| `cargo-deny` | Optional, for `cargo deny check` |

## 2. Get the code and the engines

```powershell
git clone <repository-url> quirewise
cd quirewise
pnpm -C apps/desktop/ui install --frozen-lockfile
powershell -File scripts/fetch/fetch-pdfium.ps1 -Arch x64
powershell -File scripts/fetch/fetch-qpdf.ps1 -Arch x64
```

Use `-Arch arm64` on an arm64 machine. The engines land in `vendor/` (ignored by git); the downloads are checked against the SHA-256 pins in `scripts/native-pins.json`.

Optional, only for the tools that need them (see `docs/guides/components.md`): `scripts/fetch/fetch-tessdata.ps1` and `build-tesseract.ps1` (OCR), `fetch-verapdf.ps1` (PDF/A validation, needs Java), `fetch-llama.ps1 -Model smollm2-360m-instruct` (local AI). Tests for these skip when the files are missing.

## 3. Run the app

```powershell
cd apps/desktop
.\ui\node_modules\.bin\tauri.cmd dev
```

The Tauri CLI must start from `apps/desktop`. It runs the Vite dev server and builds the Rust shell. The worker is looked up next to the app; build it once with `cargo build -p quirewise-worker` if the app reports it missing.

UI only, in a browser: `pnpm -C apps/desktop/ui dev`, then open http://localhost:5173 (no Rust side, so tools cannot run).

Command line: `cargo run -p quirewise-cli -- tools list` (binary `qw`).

## 4. Test and check

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
powershell -File scripts/checks/check-deps.ps1
pnpm -C apps/desktop/ui check
pnpm -C apps/desktop/ui lint
pnpm -C apps/desktop/ui test
```

Run one cargo command at a time. The full checklist is in `docs/AGENTS.md` section 1. Speed budgets: `cargo test --release -p quirewise-worker --test perf`.

## 5. Build installers

- NSIS installer and the staging steps: `docs/guides/installation.md`.
- MSI: `.\build-msi.ps1` (output in `dist\`).
- Portable zip: `scripts/package/make-portable.ps1 -Target x86_64-pc-windows-msvc`.

Signing and release need the owner's certificate and keys; see `docs/guides/installation.md`.

## 6. Where things are

`README.md` has the folder layout, `docs/README.md` the documentation index.

## Troubleshooting

| Symptom | Fix |
|---|---|
| "run scripts/fetch/fetch-pdfium.ps1 first" | Run the fetch scripts in step 2 |
| `tauri` cannot find the project | Start it from `apps/desktop` |
| Cargo waits on "blocking waiting for file lock" | Another cargo command is running |
| MSI build times out downloading WiX | Retry on a working connection; Tauri fetches WiX from GitHub |
