# ADR-0012: Explorer menu as classic per-user registry verbs, written with reg.exe

Status: accepted

## Context

Phase 13 asks for "Open with Quirewise" and a Quirewise submenu with a few tools in the right-click menu of PDF files. Windows 11 shows the classic menu under "Show more options"; a top-level Windows 11 entry needs a shell extension (an `IExplorerCommand` COM object) in a package with identity, which is a separate packaging and signing project.

## Decision

- **Classic verbs, current user only.** Everything goes under `HKCU\Software\Classes`, so no administrator rights are needed and nothing changes for other users. A submenu `SystemFileAssociations\.pdf\shell\Quirewise` points through `ExtendedSubCommandsKey` at `Quirewise.PdfMenu`, whose `shell` keys are the entries ("Open with Quirewise" and up to five tools). `Applications\quirewise.exe` adds Quirewise to "Open with"; Windows does not let a program take over the default PDF app silently, and Quirewise does not try.
- **reg.exe, not a registry crate.** The keys are written by `reg.exe add` and removed by `reg.exe delete`, through `std::process::Command` with each value as its own argument, so quoting is exact and the infrastructure crate keeps its single `unsafe` module. The writer sits behind a small `Registry` trait; a pure `plan` function builds every value and is tested without touching the registry, and one test runs the real `reg.exe` under a key of its own that it removes.
- **Entries start the app with a command line.** `quirewise.exe --tool <id> -- "%1"`. The parser (`domain::explorer::parse_launch`) refuses unknown options, takes everything after `--` as files and cuts the list at 500. The tool id must be a known tool.
- **One window.** `tauri-plugin-single-instance` hands the command line of a second start (Explorer starts the entry once per selected file) to the running window, which adds the files and comes to the front. A first start reads its own command line through `take_launch`.
- **Off by default, explicit, reversible.** Settings, Explorer menu adds or updates the menu with the chosen tools (installing again first removes the old entries) and removes it. The NSIS uninstaller removes the same keys. In portable mode the menu is refused, because it would write outside the app folder.
- **Text.** Menu text comes from the language of the app, is limited to 60 characters and may not hold quotes, ampersands or control characters.

## Consequences

- On Windows 11 the entry is one click deeper than a top-level one. A packaged extension can replace the registry verbs later without changing the command line contract.
- Explorer opens at most 15 selected files with a classic verb; for more, drop the files on the window.
