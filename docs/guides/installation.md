# Installation, portable mode and release

## Installer

One NSIS installer per architecture (x64 and arm64), per user, no administrator rights. WebView2 is downloaded by its bootstrapper when missing. Size budget: at most 30 MB (`docs/design/performance-budgets.md`, checked in CI).

Build it from a clean checkout:

```
powershell -File scripts/fetch-pdfium.ps1 -Arch x64
powershell -File scripts/fetch-qpdf.ps1 -Arch x64
powershell -File scripts/stage-installer.ps1 -Target x86_64-pc-windows-msvc
cd apps/desktop
.\ui\node_modules\.bin\tauri.cmd build --target x86_64-pc-windows-msvc --bundles nsis --config src-tauri/tauri.bundle.conf.json
```

`stage-installer.ps1` builds the worker in release mode and collects it, `pdfium.dll` and `qpdf` into `apps/desktop/src-tauri/staging`. `tauri.bundle.conf.json` adds those files to the bundle (the worker as a sidecar, renamed to `quirewise-worker.exe` next to the app; the engines under `native\`). The base `tauri.conf.json` does not list them, so `cargo check`, clippy and development builds work without staging.

Installed layout: `quirewise.exe`, `quirewise-worker.exe`, `native\pdfium\pdfium.dll`, `native\qpdf\qpdf.exe` and its libraries. LibreOffice, Tesseract, veraPDF, llama.cpp and the models are not part of the installer: they are components the person adds in Settings (`docs/guides/components.md`).

## Portable mode

`scripts/make-portable.ps1 -Target <triple>` zips the same files plus an empty-ish `portable.flag`. While that file sits next to the exe, settings, logs, components, recent files, recipes, saved signatures and scratch files live under `.\data` and nothing is written to the registry or the user profile. The Explorer menu is refused in portable mode. Delete the flag to use the profile folders again.

## Explorer menu

Settings, Explorer menu. See ADR-0012. The installer's uninstaller removes it.

## Signing and release

`scripts/sign.ps1` signs every exe, dll and installer with Azure Trusted Signing (`QW_SIGN_ENDPOINT`) or a certificate file (`QW_SIGN_PFX`); the secrets come from the environment and are never printed. `.github/workflows/release.yml` builds both architectures on a tag, signs, writes the SBOM and SHA-256 sums, and creates a draft release. The signing certificate, the signing service and the component manifest key are the owner's to provide (open questions 88 and 91); nothing in the repository holds them.

## MSI

`build-msi.ps1` in the repository root runs the whole chain (fetch engines, stage, `tauri build --bundles msi`) and copies the result to `dist\`. Use `-Target aarch64-pc-windows-msvc` for arm64. Tauri downloads the WiX toolset on the first run. The .msi does not remove the Explorer menu on uninstall (only the NSIS uninstaller does); turn it off in Settings first.
