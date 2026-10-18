# ADR-0001: Tauri 2 with a Rust core

Status: accepted

## Context

Quirewise is a lightweight, offline Windows desktop app for x64 and arm64. It handles hostile input files and needs memory safety in the code that coordinates parsing, plus a small installer and low idle memory.

## Decision

Use Tauri 2 as the shell (WebView2 UI, NSIS installer per architecture) and Rust stable for the core in a Cargo workspace. The UI is Svelte 5 with TypeScript, built statically with Vite. WebView2 is installed through the download bootstrapper.

## Consequences

- Small installers and low idle RAM compared with a bundled browser runtime.
- Native arm64 builds from the same code base.
- FFI to PDFium and other native libraries needs `unsafe`, confined to engine adapters with `// SAFETY:` comments.
- The WebView is untrusted: strict CSP, minimal capabilities, no remote content.
- MSI is considered only after a separate ADR confirms arm64 support.
