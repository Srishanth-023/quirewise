# ADR-0008: HTML to PDF with headless Microsoft Edge

Status: accepted

## Context

MASTER_PROMPT.md names WebView2 `PrintToPdf` in a hidden window. That needs COM calls from Rust (`unsafe`), a message loop and a window inside the confined worker, and it cannot be tested without showing UI.

## Decision

Start `msedge.exe` headless with an empty profile and drive it over the Chrome DevTools protocol on a local port: `Page.navigate`, wait for the load event and web fonts, `Page.printToPDF` with the paper, margins, scale, backgrounds and header/footer chosen. WebView2 and Edge are the same Chromium engine, and Edge is part of Windows 11 and current Windows 10.

Every request is held with the `Fetch` domain: local files, `data:` and `blob:` addresses continue, everything else is failed unless the person agreed to use the internet.

## Consequences

- No `unsafe` code, and the tests run the real browser (paper sizes, scale, blocked requests, an unreachable address, cancellation).
- The browser runs inside the worker's Job Object and is stopped with its process tree on cancel, timeout or exit.
- A computer without Edge reports `QW-COMPONENT-MISSING`.
- Pages run their scripts, so only pages the person trusts should be printed.
