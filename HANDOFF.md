# Handoff: Quirewise, Phase 13 built, release steps left

Written 2026-10-21. Read AGENTS.md and MASTER_PROMPT.md (section 15) first; CLAUDE.md only says "Follow AGENTS.md".

## What Quirewise is

Offline Windows PDF toolkit: Tauri 2 shell, Rust workspace, Svelte 5 UI, at `D:\Dev\Projects\quirewise`. 68 tools (`ToolId::ALL`). Phases 0 to 13 are built.

## Standing instruction

Work phase by phase without asking for approval. Answers: concise, no greetings, no em dashes, they/them for unknown pronouns, company and client data confidential, never print secrets. Commits end with the Co-Authored-By line from the session reminder.

## State

- `main` holds everything (Phase 13 parts 1 and 2 at `990e7e6` plus the follow-up commit). All branches are pushed to `origin`.
- Gate on `main` passed: clippy `-D warnings`, `cargo test --workspace`, `check-deps`, arm64 build, UI check, lint, visual-lint, vitest (173), perf tests in release (merge of 100 PDFs 0.43 s, first thumbnail 0.15 s).
- x64 NSIS installer built locally: 14.2 MB (budget 30 MB).

## Phase 13 map

- Recipes: `domain/recipe.rs`, `application/recipe.rs`, `infrastructure/recipes.rs` and `queue.rs` (recipe branch), CLI `recipe`, UI `features/recipes`. ADR-0011, `docs/tools/recipes.md`.
- Explorer menu: `domain/explorer.rs`, `infrastructure/explorer.rs` (reg.exe, HKCU), single instance + `launch` event, Settings, Explorer menu. ADR-0012.
- Installer and portable: `scripts/stage-installer.ps1`, `tauri.bundle.conf.json` (overlay), `installer/hooks.nsh`, `scripts/make-portable.ps1`. Build from `apps/desktop` with `ui\node_modules\.bin\tauri.cmd build --config src-tauri/tauri.bundle.conf.json` (see `docs/installation.md`).
- i18n and a11y: logical CSS (visual-lint rule `physical-direction`), `formatNumber`, axe tests, live region in RunBar.

## Left

1. Build the portable zip once (`scripts/make-portable.ps1`), never run.
2. Install-test the installer on a clean machine; build arm64 installer (open question 104).
3. Signing and release need the owner's certificate, signing service and component manifest key (open questions 88, 91, 103). Then tag and run `release.yml`.
4. Open questions 102 (Windows 11 "Show more options") to 104.

## Gotchas

- Bash heredocs and Python mangle backslashes: use Write or Edit for code with `\`.
- One cargo command at a time. `cargo fmt` rewraps lines.
- The tauri CLI must run from `apps/desktop`.
- Foreground `sleep` is blocked.
