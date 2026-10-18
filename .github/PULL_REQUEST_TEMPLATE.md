## What

## Why

## How tested

- [ ] x64
- [ ] arm64

## Screenshots of real output

## License impact

New dependencies, their licenses, and why each is needed.

## Security impact

Files, network, crypto, IPC or permissions touched.

## Checklist

- [ ] Matches the Appendix A row: all options present, defaults correct
- [ ] Tests added or updated and passing on x64 and arm64
- [ ] fmt, clippy (deny warnings), TS check, lint, visual lint, check-deps, cargo-deny clean
- [ ] Layer boundaries respected
- [ ] Inputs validated; errors use catalog codes with actionable messages
- [ ] No network, telemetry or new permission without a consent flow and ADR
- [ ] Outputs verified; inputs unchanged; temp files cleaned
- [ ] Cancellation and progress work; memory within budget on the large fixture
- [ ] UI: tokens only, no banned patterns, keyboard and screen reader usable, real previews, at most 3 actions
- [ ] No em dashes, emojis, banned fonts or banned icons in the diff
- [ ] Docs, CHANGELOG, THIRD_PARTY_NOTICES, ADRs updated
- [ ] Diff self-reviewed; commit message follows Conventional Commits
