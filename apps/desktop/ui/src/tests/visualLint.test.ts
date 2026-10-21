import { describe, expect, it } from 'vitest';
import { lintText } from '../../scripts/visualLintRules.mjs';

const rules = (path: string, text: string) => lintText(path, text).map((f) => f.rule);

describe('visual lint rules', () => {
  it('flags banned style patterns', () => {
    expect(rules('a.css', 'a { box-shadow: 0 1px 2px red; }')).toContain('shadow');
    expect(rules('a.css', 'a { filter: drop-shadow(0 0 2px red); }')).toContain('shadow');
    expect(rules('a.css', 'a { background: linear-gradient(red, blue); }')).toContain('gradient');
    expect(rules('a.css', 'a { backdrop-filter: blur(4px); }')).toContain('blur');
    expect(rules('a.css', 'a { font-family: Inter, sans-serif; }')).toContain('font');
    expect(rules('a.css', 'a { color: white; }')).toContain('white');
    expect(rules('a.css', 'a { color: #1f2328; }')).toContain('raw-hex');
  });

  it('allows raw hex only inside tokens.css, and never pure white', () => {
    expect(rules('src/lib/design/tokens.css', ':root { --bg: #f4f2ee; }')).toEqual([]);
    expect(rules('src/lib/design/tokens.css', ':root { --bg: #FFFFFF; }')).toContain('white');
  });

  it('limits border radius to 4 px', () => {
    expect(rules('a.css', 'a { border-radius: 2px; }')).toEqual([]);
    expect(rules('a.css', 'a { border-radius: 4px; }')).toEqual([]);
    expect(rules('a.css', 'a { border-radius: 8px; }')).toContain('radius');
    expect(rules('a.css', 'a { border-radius: 50%; }')).toContain('radius');
    expect(rules('a.css', 'a { border-radius: var(--radius-control); }')).toEqual([]);
  });

  it('flags em dashes, emoji and Lucide imports', () => {
    expect(rules('a.md', 'a — b')).toContain('em-dash');
    expect(rules('a.svelte', '<p>\u{1F680}</p>')).toContain('emoji');
    expect(rules('a.ts', "import { Pdf } from 'lucide-svelte';")).toContain('lucide');
  });

  it('asks for logical properties so layouts work right to left', () => {
    expect(rules('a.css', 'a { margin-left: 4px; }')).toContain('physical-direction');
    expect(rules('a.css', 'a { padding-right: 4px; }')).toContain('physical-direction');
    expect(rules('a.css', 'a { border-left-color: var(--focus); }')).toContain('physical-direction');
    expect(rules('a.css', 'a { text-align: left; }')).toContain('physical-direction');
    expect(rules('a.css', 'a { float: right; }')).toContain('physical-direction');
    expect(rules('a.css', 'a { margin-inline-start: 4px; text-align: start; }')).toEqual([]);
  });

  it('accepts clean token-based styles', () => {
    expect(rules('a.svelte', 'a { color: var(--text); border: 1px solid var(--border); }')).toEqual([]);
  });
});
