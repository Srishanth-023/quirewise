# Threat model (STRIDE draft)

Updated for any feature that touches files, network or cryptography.

## Assets

User documents, passwords, signing keys, local files outside the ones the user chose.

## Trust boundaries

1. Input files (hostile) to the worker process.
2. Worker process to the core (stdio IPC).
3. UI (WebView2) to the core (Tauri commands).
4. Core to on-demand components and sidecars.
5. Core to the network (off by default, consent per action).

## STRIDE summary

| Threat | Example | Control |
|---|---|---|
| Spoofing | Tampered component download | Pinned SHA-256 and Ed25519 signature on `components.json`; verify before extraction |
| Tampering | Output overwrites an input; partial output | Never write to input path; temp file, verify, atomic rename |
| Repudiation | Not applicable for a single-user offline app | Local consent log for network actions |
| Information disclosure | Content or passwords in logs or temp files | No content in logs, hashed paths, `zeroize` for secrets, per-session temp dir with restrictive ACL wiped on exit |
| Denial of service | Decompression bomb, huge page count, endless loop | Job Object memory and CPU caps, limits on ratio, objects and pages, per-job timeout, cancellation per page |
| Elevation of privilege | Parser memory bug, launch action, PDF JavaScript, injected web content calling IPC | Parse only in the worker under Job Object; never run JavaScript or follow launch or URI actions; strict CSP and minimal Tauri capabilities; no remote content in the WebView |

## Path handling

Canonicalize paths, reject device paths and reparse-point escapes, allow UNC only when the user picked it, sanitize generated names (reserved names CON, PRN, AUX, NUL, COM1-COM9, LPT1-LPT9, trailing dots and spaces, illegal characters, length limits).

## Open items

Tracked in `docs/open-questions.md`.
