# Architecture

Quirewise follows Clean Architecture. Dependencies point inward only.

```
ui -> app-shell -> application -> domain
engines/*, infrastructure  implement  application ports
worker  hosts engines and runs tool plans in an isolated process
```

## Layers

| Layer | Crate or folder | Rules |
|---|---|---|
| domain | `crates/domain` | Pure types and rules, no I/O. Allowed external crates: serde, schemars, thiserror |
| application | `crates/application` | Use cases and ports (traits). Depends only on domain |
| engines | `crates/engines/*` | Adapters for ports. Never import each other or infrastructure |
| infrastructure | `crates/infrastructure` | File store, settings, logging, components, job queue, Windows integration |
| worker | `crates/worker` | Only process that parses untrusted PDF bytes. Runs inside a Windows Job Object |
| app-shell | `apps/desktop/src-tauri` | Thin Tauri adapter: commands map DTOs to use cases. No PDF logic |
| ui | `apps/desktop/ui` | Svelte 5. Never touches the file system, calls typed commands only |

`scripts/checks/check-deps.ps1` enforces the allowed edges using `cargo metadata` and runs in CI.

## Process model

The UI (WebView2) calls typed Tauri commands. The core dispatches jobs to `quirewise-worker.exe`, which streams progress over stdio IPC. Heavy engines (LibreOffice, Tesseract, llama.cpp, veraPDF, optional Ghostscript) run as separate sidecar processes and are never linked in-process.

## Tool contract

Every tool implements `Tool` (id, options schema, validate, execute) and is registered in one `ToolRegistry`. The UI and CLI read the registry and never hardcode tool lists. Option forms are generated from the JSON Schema.

## Data flow for a job

1. UI sends a typed command with input paths and options.
2. The `RunTool` use case validates inputs and options (domain) and builds a plan.
3. The job queue hands the plan to a worker process.
4. The worker writes a temp file next to the destination, verifies it, then renames atomically.
5. Progress and completion events return to the UI through the event bridge.

See `docs/adr/` for decisions and `docs/tools/<tool-id>.md` for per-tool details.

## Queries and typed IPC (Phase 2)

Read-only questions (inspect, thumbnails) travel as `DocumentQuery` to a persistent worker session (ADR-0007). The UI talks to the shell through typed commands whose DTOs are generated from Rust JSON Schema (ADR-0006). Native engines are located by `NativePaths` (the installed `native/` folder, or `vendor/` in development).
