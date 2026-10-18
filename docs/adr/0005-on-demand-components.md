# ADR-0005: Signed on-demand components

Status: accepted

## Context

LibreOffice, language data, veraPDF with a JRE, llama.cpp and GGUF models are large and optional. The installer budget is 30 MB per architecture.

## Decision

Ship a signed `components.json` manifest listing id, version, architecture, URL, size, SHA-256 and an Ed25519 (minisign) signature. Download only after explicit consent, verify hash and signature before extraction, install to `%LOCALAPPDATA%\Quirewise\components\<id>\<version>\`, allow removal from Settings and offline import from a local `.qwpkg` file. Show a model license before its download.

## Consequences

- The app works fully offline once components are installed.
- The manifest parser is a fuzz target.
- A tampered or mismatched component fails with `QW-COMPONENT-VERIFY-FAILED`.
