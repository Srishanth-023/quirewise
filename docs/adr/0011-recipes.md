# ADR-0011: Recipes run through the normal unit runner, one file at a time

Status: accepted

## Context

Appendix A row 58 asks for chained tools that can be saved, exported, imported and run on files or a folder. Every tool already runs as a unit in the worker with validation, a verified output and the never-overwrite rule.

## Decision

- **Model.** A recipe is a list of steps; each step is the options of one tool (the options carry the tool tag). The domain knows only the chain rules: every tool has a `ChainRole` (source, middle, last, excluded) and a chain is valid when sources come first, last-role tools come last and nothing is excluded.
- **Runner.** `run_recipe_unit` in the application layer calls the existing `UnitRunner` once per step, so each step is a normal unit in the worker (limits, job object, verification) and a failure names its step. The batch queue gets an optional recipe in its request and runs the chain per file, so status, retry of failed files, cancel and events work as for a single tool.
- **Files between steps.** A step that is not the last writes into its own folder in the session scratch space with the pattern `{filename}`, so the name stays clean for the last step. The files are removed after each step and when a step fails. A middle step must write exactly one file.
- **No new worker protocol.** The worker is unchanged; a recipe is only a sequence of messages it already understands.
- **Storage.** One JSON file in the roaming app data folder (`recipes.json`; `data\recipes.json` in portable mode). A damaged file is renamed to `.damaged`, not overwritten. Export and import go through the system file dialogs opened by the Rust side, so web content never names a path.
- **Folder scan.** Non-recursive by default, never follows links, junctions or hidden entries, capped at depth 8 and 2000 files.
- **UI.** Each step has its own option store and uses the same schema-driven form as the tool screens, so a new tool or option appears in recipes without UI work.

## Consequences

- A recipe step cannot use per-file or editor-only options; those tools are excluded.
- Steps run one after another per file, but files run in parallel as in any batch.
- The worker is started per unit by the supervisor as before, so a recipe of N steps costs N units of overhead.
