# ADR-0002: Clean Architecture with an isolated worker process

Status: accepted

## Context

Tools must be testable without engines, and untrusted PDFs must not be parsed in the UI-facing process.

## Decision

Keep dependencies pointing inward (`ui -> app-shell -> application -> domain`). Engines and infrastructure implement application ports. A separate `quirewise-worker.exe` loads untrusted PDFs and runs tool plans inside a Windows Job Object (memory, CPU time, no child processes except allowed sidecars). Heavy engines run as sidecar processes. `scripts/check-deps.ps1` enforces the allowed edges in CI.

## Consequences

- A parser crash or runaway job kills only the worker.
- Use cases are tested with fake ports.
- IPC protocol types live in `application` so the supervisor and the worker share them.
- Extra process start cost is paid per job batch, not per page.
