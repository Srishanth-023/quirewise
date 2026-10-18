# AGENTS.md: Always-on rules for Quirewise

`CLAUDE.md` contains one line: "Follow AGENTS.md". This file is the single source of truth for rules. Scope, stack, phases and the normative feature matrix (Appendix A) live in `MASTER_PROMPT.md`.

## 0. Mission
Build and maintain Quirewise, an offline-first Windows PDF toolkit for x64 and arm64 with full iLovePDF + Sejda parity. These rules apply to every task, file and commit.

## 1. Workflow for every task
1. Read the relevant `MASTER_PROMPT.md` section, the matching Appendix A row(s), and `docs/tools/<tool-id>.md` if it exists.
2. Write or update tests first (unit, integration, golden, negative).
3. Implement the smallest change that passes, respecting layer boundaries.
4. Run:
   - `cargo fmt --all`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `cargo deny check`
   - `pwsh scripts/check-deps.ps1`
   - `pnpm -C apps/desktop/ui check`
   - `pnpm -C apps/desktop/ui lint`
   - `pnpm -C apps/desktop/ui test`
5. Build both targets: `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`.
6. Update docs, CHANGELOG, THIRD_PARTY_NOTICES (if dependencies changed), ADR (if a decision changed).
7. Self-review the full diff against section 12, then commit.

Never stop to ask for confirmation during a planned phase. Record open questions in `docs/open-questions.md` and continue with the safest reasonable default. Never implement anything listed as out of scope in `MASTER_PROMPT.md` section 3.2.

## 2. Architecture boundaries (hard)
- `domain` depends on nothing project-internal and performs no I/O.
- `application` depends only on `domain` and defines ports (traits).
- `engines/*` and `infrastructure` implement ports; they never import each other directly.
- `worker` hosts engines and is the only process that parses untrusted PDF bytes.
- `apps/desktop/src-tauri` is a thin adapter: commands map DTOs to use cases. No PDF logic.
- `ui` never touches the file system directly; it calls typed commands only.
- GPL/AGPL code only behind opt-in Cargo features, in separate processes, never in default builds.
- `scripts/check-deps.ps1` (using `cargo metadata`) fails CI on any forbidden edge.

## 3. Coding conventions
- **Rust:** pinned stable toolchain. `#![forbid(unsafe_code)]` in `domain`, `application`, `infrastructure`. `unsafe` only in engine FFI modules, each block with a `// SAFETY:` comment. Clippy `unwrap_used` and `expect_used` denied outside tests. Errors via `thiserror`; app boundary maps to `AppError` with codes from `MASTER_PROMPT.md` Appendix C.
- **Types:** newtypes for units (`PageIndex`, `PageNumber`, `Dpi`, `Percent`, `Bytes`, `Millimeters`). Option structs derive `serde` + `schemars`, with defaults matching Appendix A, validated in `domain`.
- **Concurrency:** async only for I/O; CPU work on bounded pools; every long operation takes a `CancellationToken` and reports progress.
- **Resources:** RAII wrappers for PDFium documents and pages; never share handles across threads unless the binding is thread-safe.
- **TypeScript:** `strict: true`; no `any`; no non-null assertions without a comment; Svelte 5 runes; components under 200 lines; one feature folder per tool.
- **Naming:** tool ids kebab-case; Rust modules snake_case; types PascalCase; TS files camelCase; Svelte components PascalCase.svelte.
- No dead code, no commented-out code, no TODO without an issue or `docs/open-questions.md` reference.
- Comments explain why, not what. Every public item has rustdoc or TSDoc.
- **No em dash (U+2014)** anywhere: code, comments, UI strings, docs, commit messages. Use commas, colons, parentheses or hyphens.

## 4. UI rules (lint-enforced)
**Forbidden:** harsh gradients; Lucide icons (use Phosphor Regular only); pure white (`#FFFFFF`, `white`); rainbow or multi-hue decorative color; `box-shadow` or `drop-shadow`; three feature cards in a row; emojis; liquid glass or `backdrop-filter` blur; Inter, Geist, Space Grotesk; colored left-stripe borders on cards or alerts; fake testimonials; bento grids; terminal-window mockups; "It's not X, it's Y" phrasing; check-mark bullets; placeholder or fake previews; `border-radius` above 4 px; purple-and-black palettes; skeleton loaders; radial orbs; dot-grid backgrounds; sparkle icons (AI features use the plain text label "Local AI"); animated arrows; neon colors; missing Terms, Privacy, About, License screens.

**Required:** design tokens from `lib/design/tokens.css` only (no raw hex in components); 1 px borders for separation; follow Windows light/dark; `forced-colors` support; 2 px focus ring in `--focus` with 2 px offset; WCAG 2.2 AA contrast; targets at least 24x24 px; everything keyboard reachable; determinate progress with counts; crisp at any DPI; usable from 900x600 to 4K and at 200% zoom.

**Copy style:** plain, specific, sentence case; verbs on buttons ("Compress 3 files"); errors say what happened and what to do next.

## 5. Minimal Touch Points (MTP)
- Any tool completes in at most 3 actions from a dropped file using defaults.
- Drag and drop on every screen. Ctrl+K command palette. Ctrl+O open. Enter runs the current tool. Esc closes dialogs or cancels a running job (with confirm).
- Remember last options per tool; show "Reset to defaults".
- Suggest the likely tool from the input type.
- Completion panel: Open, Show in folder, Run another tool on result.
- No confirmation dialog unless the action is destructive and irreversible. Outputs never overwrite inputs, so most actions need none.

## 6. Security and privacy
- Treat every input file as hostile. Parse only in the worker under Job Object limits, with caps on object count, decompression ratio, page count, and time.
- No network by default. Any network use (URL to PDF, component download, TSA timestamp, revocation check, update check, loopback AI endpoint) requires explicit consent in that flow and is logged locally.
- Zero telemetry, analytics, remote fonts or remote scripts. Keep the CSP from `MASTER_PROMPT.md` section 12; Tauri capabilities minimal per window; asset protocol scoped to the session temp directory.
- Never execute PDF JavaScript. Never follow launch or URI actions.
- Passwords and keys: memory only, `zeroize` on drop, never logged, never written to settings. Saved visual signatures encrypted with DPAPI.
- Paths: canonicalize; deny traversal and reserved device names; sanitize generated file names; temp files only in `%TEMP%\Quirewise\<session>`, deleted afterwards.
- Components install only if SHA-256 and Ed25519 signature match the pinned manifest. Show model licenses before AI model downloads.
- Redaction removes content; verify by re-extracting text and images in redacted regions.

## 7. Data integrity
- Never write to an input path. Temp file in destination folder, flush, verify, atomic rename.
- Before reporting success: reopens in PDFium, passes `qpdf --check`, expected page count, tool-specific assertions (encryption, PDF/A report, signature validity, absence of redacted text).
- On failure: delete temp output, leave input untouched, show error code and next step.

## 8. Testing
- Each tool: option validation unit tests; CLI integration tests on fixtures; golden fingerprints; negative tests (encrypted, damaged, empty, zero-page, huge, malicious); cancellation test; both arches.
- Each bug fix: a regression test named `regression_<slug>`.
- Parsers and IPC payloads: `proptest` + `cargo-fuzz` targets.
- UI: component tests, Playwright E2E for MTP flows, axe-core with zero violations, visual lint clean.
- Never delete or weaken a failing test to pass CI. Fix the cause, or quarantine with a linked issue.
- Golden files change only via `scripts/update-golden.ps1`, with render diff images in the PR.

## 9. Dependencies
- Add a dependency only if maintained (release within 12 months), license-allowed, and arm64-compatible. State the reason in the PR.
- Allowed licenses: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, MPL-2.0, OFL-1.1, Unicode-3.0, CC0-1.0. Anything else needs an ADR and must be opt-in.
- Lockfiles committed and used (`--locked`, `pnpm install --frozen-lockfile`). Update THIRD_PARTY_NOTICES.md.
- Native binaries (PDFium, qpdf, Tesseract) are fetched by scripts with pinned versions and SHA-256, never committed.

## 10. Commits, branches, PRs
- Conventional Commits: `feat(split): add split by size`, `fix(worker): cap object count`, plus `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`. Breaking changes use `!` and a footer.
- Small focused commits; each builds and passes tests.
- Branches: `feat/<tool-id>`, `fix/<slug>`, `chore/<slug>`.
- PR template: What, Why, How tested (x64 and arm64), Screenshots of real output, License impact, Security impact, Checklist.
- CHANGELOG in Keep a Changelog format; SemVer bump on release.

## 11. How to add a new tool
1. Add the `ToolId` variant and options struct (defaults from Appendix A) in `crates/domain/src/tools/<tool_id>.rs`; implement validation and schema.
2. Wire the use case in `application` (no engine imports). Add any needed port method there first.
3. Implement the plan executor in the right engine adapter(s).
4. Register in `ToolRegistry`; add the CLI subcommand.
5. Add fixtures, golden fingerprints, unit, integration, negative and cancellation tests.
6. Add `ui/src/lib/features/<tool-id>/` using generated option types; real preview; MTP-compliant.
7. Add i18n strings, `docs/tools/<tool-id>.md`, CHANGELOG entry, notices if needed.
8. Run the checklist below.

## 12. Checklist before marking any task done
- [ ] Matches the Appendix A row: all options present, defaults correct.
- [ ] Tests added/updated and passing on x64 and arm64.
- [ ] fmt, clippy (deny warnings), TS check, lint, visual lint, check-deps, cargo-deny all clean.
- [ ] Layer boundaries respected.
- [ ] Inputs validated; errors use catalog codes with actionable messages.
- [ ] No network, telemetry, or new permission without a consent flow and ADR.
- [ ] Outputs verified; inputs unchanged; temp files cleaned.
- [ ] Cancellation and progress work; memory within budget on the large fixture.
- [ ] UI: tokens only, no banned patterns, keyboard and screen reader usable, real previews, at most 3 actions.
- [ ] No em dashes, emojis, banned fonts or banned icons in the diff.
- [ ] Docs, CHANGELOG, THIRD_PARTY_NOTICES, ADRs updated.
- [ ] Diff self-reviewed; commit message follows Conventional Commits.
