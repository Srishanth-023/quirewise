# Changelog

All notable changes are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Phase 2: PDFium, qpdf and lopdf engine adapters (open, render, text, facts, checks, page copy, structure), persistent worker session for queries, Inspect and Properties, recent files, typed IPC generated from Rust JSON Schema, and the app shell with real page thumbnails, window-wide drop, command palette, batch queue and progress, inline password prompt, keyboard shortcuts and settings. Native binaries are fetched with pinned SHA-256.
- Phase 1: domain rules (units, page range grammar, naming tokens and sanitizing, path policy, error catalog, settings, safety limits, component manifest), application ports, `Tool` contract and `ToolRegistry`, `RunTool` use case, batch retry semantics, worker IPC protocol, atomic never-overwriting file store, settings with migrations, JSON logging, job queue, worker process with Job Object confinement, supervisor, `qw` CLI and fuzz targets.
- Phase 0: repository scaffold, Cargo workspace, Svelte UI shell, Ink and Paper design tokens, visual lint, CI workflows, ADR-0001 to ADR-0005 and the threat model draft.
