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

## Local AI (Phase 12)

- **Data.** Text of documents goes to a language model. The model is a process on this computer reached only on the loopback address (parsed so that look-alike addresses are refused), without a proxy. The built-in server also needs a random key that only the worker knows (environment, not command line). Ollama is used only after the person agreed in the options of that run.
- **Prompt injection.** A document can say "ignore your instructions". Every prompt marks the text as data, and no answer is trusted: page numbers, titles, personal details, translations and tidied text are parsed and checked against the document before use; the model has no tools and cannot write files. The worst result of a manipulated answer is a wrong split, summary or translation, which the person sees; Redact only removes strings found on the pages, and the preview lists them first.
- **Resources.** A server can use all memory and processor time. It is started outside the worker's caps (those suit parsing, not inference) in a job that ends it when the worker ends; the worker's own job allows breakaway only for a child that asks for it. A crash of the worker therefore does not leave a server behind.
- **Models.** Model files are downloaded as components with a hash check, after the license is shown. A model file is parsed by llama.cpp, which is not part of the PDF parsing path.
