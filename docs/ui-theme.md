# UI theme: Ink and Paper

Tokens live in `apps/desktop/ui/src/lib/design/tokens.css`, the only file allowed to hold raw hex colors. The values mirror `MASTER_PROMPT.md` section 7.

## Rules

- Follow the Windows light or dark theme (`prefers-color-scheme`); a manual override sets `data-theme` on the root element. `forced-colors: active` maps tokens to system colors.
- Fonts: IBM Plex Sans (UI) and IBM Plex Mono (numbers, ranges, file sizes), bundled as subsetted WOFF2. Scale 12, 13, 14 (base), 16, 20, 24 px.
- Icons: Phosphor Regular at 16 and 20 px with `currentColor`. Import icons one by one in `lib/components/icons.ts`.
- Spacing scale 2, 4, 8, 12, 16, 24, 32, 48 px. Radius 2 px controls, 4 px panels, 0 px thumbnails.
- No shadows, no gradients, no blur. Separate with 1 px borders and surface steps.
- Motion 120 to 160 ms ease-out for state changes only, off under `prefers-reduced-motion`.
- Progress is determinate with counts. Spinners only for waits under 500 ms. No skeleton loaders.
- Focus ring: 2 px in `--focus` with 2 px offset. Targets at least 24 by 24 px.
- Layout: left navigation, central workspace, right options panel. One column below 1100 px.

## CSP constraint

The CSP uses `style-src 'self'`, so markup must not use inline `style="..."` attributes. Use classes or Svelte `style:` directives, which set styles through the CSSOM.

## Enforcement

`pnpm -C apps/desktop/ui lint` runs ESLint and `scripts/visual-lint.mjs`. The latter fails on shadows, gradients, blur, banned fonts, Lucide imports, emoji, em dashes, radius above 4 px, raw hex outside tokens and pure white.
