import { describe, expect, it } from 'vitest';
import { addable, baseName, canAppend, chainProblem, startsWithSource, type Roles } from '../lib/features/recipes/model';

const roles: Roles = {
  'word-to-pdf': 'source',
  rotate: 'middle',
  compress: 'middle',
  protect: 'last',
  'split-every-n': 'last',
  merge: 'excluded',
};

describe('recipe chain rules', () => {
  it('accepts a source, middle steps, then a last step', () => {
    expect(chainProblem(['word-to-pdf', 'rotate', 'compress', 'protect'], roles)).toBeNull();
    expect(chainProblem(['rotate'], roles)).toBeNull();
  });

  it('names the first step that breaks a rule', () => {
    expect(chainProblem(['merge'], roles)).toEqual({ step: 1, reason: 'excluded' });
    expect(chainProblem(['rotate', 'word-to-pdf'], roles)).toEqual({ step: 2, reason: 'source-first' });
    expect(chainProblem(['protect', 'rotate'], roles)).toEqual({ step: 1, reason: 'last-only' });
    expect(chainProblem(['rotate', 'unknown-tool'], roles)).toEqual({ step: 2, reason: 'excluded' });
  });

  it('offers only what can follow the current steps', () => {
    const list = Object.keys(roles).map((id) => ({ id }));
    const ids = (steps: string[]) => addable(list, steps, roles).map((t) => t.id);
    expect(ids([])).toEqual(['word-to-pdf', 'rotate', 'compress', 'protect', 'split-every-n']);
    expect(ids(['rotate'])).toEqual(['rotate', 'compress', 'protect', 'split-every-n']);
    expect(ids(['rotate', 'protect'])).toEqual([]);
    expect(canAppend(['word-to-pdf'], 'word-to-pdf', roles)).toBe(false);
  });

  it('knows when a folder run should also take Office files', () => {
    expect(startsWithSource(['word-to-pdf', 'rotate'], roles)).toBe(true);
    expect(startsWithSource(['rotate'], roles)).toBe(false);
    expect(startsWithSource([], roles)).toBe(false);
  });

  it('shows a file by its name on either kind of separator', () => {
    expect(baseName('C:\\docs\\a.pdf')).toBe('a.pdf');
    expect(baseName('docs/b.pdf')).toBe('b.pdf');
  });
});
