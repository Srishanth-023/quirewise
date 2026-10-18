import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// Vitest runs from the UI package folder.
const css = readFileSync(resolve(process.cwd(), 'src/lib/design/tokens.css'), 'utf8');

/** Reads `--name: #hex;` pairs from one block of tokens.css. */
function readTokens(block: string): Record<string, string> {
  const tokens: Record<string, string> = {};
  for (const m of block.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-fA-F]{6});/g)) {
    tokens[m[1] as string] = m[2] as string;
  }
  return tokens;
}

const light = readTokens(css.slice(css.indexOf(':root {'), css.indexOf('@media (prefers-color-scheme: dark)')));
const dark = readTokens(css.slice(css.indexOf(":root[data-theme='dark']"), css.indexOf('@media (forced-colors')));

function luminance(hex: string): number {
  const channel = (i: number): number => {
    const v = parseInt(hex.slice(1 + i * 2, 3 + i * 2), 16) / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2);
}

/** WCAG 2 contrast ratio between two hex colors. */
function ratio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

const backgrounds = ['bg', 'surface', 'surface-2', 'selection'] as const;
const textColors = ['text', 'text-muted'] as const;
const statusColors = ['accent', 'success', 'warning', 'danger'] as const;

describe.each([
  ['light', light],
  ['dark', dark],
])('%s theme contrast (WCAG 2.2 AA)', (_name, tokens) => {
  it('defines every token it is checked against', () => {
    for (const key of [...backgrounds, ...textColors, ...statusColors, 'on-accent', 'focus', 'control-border']) {
      expect(tokens[key], key).toMatch(/^#[0-9a-fA-F]{6}$/);
    }
  });

  it('body and muted text reach 4.5:1 on every background', () => {
    for (const fg of textColors) {
      for (const bg of backgrounds) {
        expect(ratio(tokens[fg] as string, tokens[bg] as string), `${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('accent and status colors reach 4.5:1 as text on bg and surface', () => {
    for (const fg of statusColors) {
      for (const bg of ['bg', 'surface'] as const) {
        expect(ratio(tokens[fg] as string, tokens[bg] as string), `${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('button text on the accent fill reaches 4.5:1', () => {
    expect(ratio(tokens['on-accent'] as string, tokens['accent'] as string)).toBeGreaterThanOrEqual(4.5);
  });

  it('focus ring and control borders reach 3:1 against the page', () => {
    for (const bg of ['bg', 'surface'] as const) {
      expect(ratio(tokens['focus'] as string, tokens[bg] as string), `focus on ${bg}`).toBeGreaterThanOrEqual(3);
      expect(ratio(tokens['control-border'] as string, tokens[bg] as string), `control border on ${bg}`).toBeGreaterThanOrEqual(3);
    }
  });
});
