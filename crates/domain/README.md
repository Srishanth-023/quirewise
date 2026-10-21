# quirewise-domain

Pure types and rules: units (`PageIndex`, `PageNumber`, `Dpi`, `Percent`, `Bytes`, `Millimeters`), the page range grammar, output naming tokens and sanitizing, path text policy, error codes, per-tool option structs, job statuses, settings and safety limits.

No I/O. External crates allowed: serde, schemars, thiserror (enforced by `scripts/checks/check-deps.ps1`).
