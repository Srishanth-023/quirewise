# ADR-0006: Typed IPC generated from JSON Schema

Status: accepted

## Context

The UI must call the Rust core through typed commands, and the types must come from Rust so the two sides cannot drift. `tauri-specta` is the usual choice, but it tracks Tauri release candidates and would couple our build to its release schedule. The domain and application crates already derive `schemars::JsonSchema` for option forms.

## Decision

Every DTO in `application::dto` derives `JsonSchema`. `qw schema` prints one combined schema (`IpcTypes`). `pnpm gen:types` runs it and compiles TypeScript with `json-schema-to-typescript` into `src/lib/ipc/generated.ts`, which is committed. CI regenerates the file and fails on any difference. `client.ts` wraps each Tauri command in a typed function and normalizes every failure into an `AppError`.

## Consequences

- One source of truth: Rust types. Option forms and IPC types come from the same schema pipeline.
- Command names and argument shapes in `client.ts` are written by hand, so a mismatch with Rust shows up as a failing command in the UI tests and in the app, not at compile time. Revisit `tauri-specta` when it is stable.
- Fields with a `default` generate a few duplicate interfaces (`ImageSummary1`). They are structurally identical and unused.
- `IpcTypes` fields carry no doc comments, because a description next to a `$ref` makes the generator emit duplicates.
