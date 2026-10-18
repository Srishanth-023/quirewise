import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/svelte';
import CommandPalette from '../lib/components/CommandPalette/CommandPalette.svelte';
import { rankCommands, type Command } from '../lib/components/CommandPalette/commands';

const cmd = (id: string, label: string, run = () => undefined): Command => ({
  id,
  group: 'tools',
  label,
  run,
});

describe('rankCommands', () => {
  const all = [cmd('1', 'Compress'), cmd('2', 'Extract pages'), cmd('3', 'Rotate'), cmd('4', 'Protect')];

  it('returns everything for an empty query', () => {
    expect(rankCommands(all, '  ')).toHaveLength(4);
  });

  it('ranks prefixes before substrings before subsequences', () => {
    const ranked = rankCommands(all, 'ro').map((c) => c.label);
    expect(ranked).toEqual(['Rotate', 'Protect']);
    expect(rankCommands(all, 'cmp').map((c) => c.label)).toEqual(['Compress']);
    expect(rankCommands(all, 'zzz')).toEqual([]);
  });
});

describe('CommandPalette', () => {
  it('filters as you type and runs the highlighted command on Enter', async () => {
    const run = vi.fn();
    const onclose = vi.fn();
    render(CommandPalette, {
      open: true,
      onclose,
      commands: [cmd('a', 'Compress'), cmd('b', 'Rotate', run)],
    });
    const input = await screen.findByRole('combobox');
    await fireEvent.input(input, { target: { value: 'rot' } });
    expect(screen.queryByRole('option', { name: /Compress/ })).toBeNull();
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onclose).toHaveBeenCalled();
    expect(run).toHaveBeenCalledOnce();
  });

  it('moves the highlight with the arrow keys and wraps around', async () => {
    render(CommandPalette, {
      open: true,
      onclose: () => undefined,
      commands: [cmd('a', 'Alpha'), cmd('b', 'Beta')],
    });
    const input = await screen.findByRole('combobox');
    expect(screen.getByRole('option', { name: /Alpha/ }).getAttribute('aria-selected')).toBe('true');
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(screen.getByRole('option', { name: /Beta/ }).getAttribute('aria-selected')).toBe('true');
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(screen.getByRole('option', { name: /Alpha/ }).getAttribute('aria-selected')).toBe('true');
  });

  it('says so when nothing matches', async () => {
    render(CommandPalette, { open: true, onclose: () => undefined, commands: [cmd('a', 'Alpha')] });
    const input = await screen.findByRole('combobox');
    await fireEvent.input(input, { target: { value: 'qqq' } });
    expect(screen.getByText('Nothing matches.')).toBeTruthy();
  });
});
