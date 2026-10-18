# Security policy

## Reporting a vulnerability

Report vulnerabilities privately through GitHub Security Advisories on the repository. Do not open a public issue for a vulnerability. Include the Quirewise version, Windows version and architecture, and a minimal file or steps that reproduce the problem.

## Scope

Quirewise treats every input document as hostile. Parser crashes, sandbox escapes from the worker process, path traversal through output names, component verification bypasses and IPC abuse are in scope. See `docs/threat-model.md`.

## Supported versions

Only the latest release receives security fixes.
