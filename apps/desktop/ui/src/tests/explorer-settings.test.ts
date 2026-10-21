import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { rotateSchema } from './fixtures';

const ipc = vi.hoisted(() => ({
  explorerStatus: vi.fn(),
  explorerTools: vi.fn(),
  explorerEnable: vi.fn(),
  explorerDisable: vi.fn(),
  getSettings: vi.fn(),
  listTools: vi.fn(),
}));

vi.mock('../lib/ipc/client', () => ({ ipc, toAppError: (e: unknown) => e }));

import Explorer from '../lib/features/settings/Explorer.svelte';
import { settings } from '../lib/stores/settings.svelte';
import { tools } from '../lib/stores/tools.svelte';

const ids = ['rotate', 'compress', 'merge', 'ocr', 'flatten', 'sanitize'];

beforeEach(async () => {
  vi.resetAllMocks();
  ipc.listTools.mockResolvedValue(
    ids.map((id) => ({ id, group: 'edit', label: id, input_mode: 'per-file', options_schema: rotateSchema })),
  );
  ipc.explorerTools.mockResolvedValue(ids);
  ipc.explorerStatus.mockResolvedValue(false);
  ipc.getSettings.mockResolvedValue({
    theme: 'system',
    remember_recent_files: true,
    explorer_tools: ['compress', 'merge'],
  });
  await tools.load();
  await settings.load();
});

async function violations(): Promise<string[]> {
  const result = await axe.run(document.body, {
    rules: { 'color-contrast': { enabled: false }, region: { enabled: false } },
  });
  return result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html.slice(0, 80)).join(' | ')}`);
}

describe('Explorer menu settings', () => {
  it('starts from the saved tools, adds the menu with their names, and is accessible', async () => {
    ipc.explorerEnable.mockResolvedValue(undefined);
    render(Explorer);
    await screen.findByText('The Explorer menu is off.');
    const boxes = (await screen.findAllByRole('checkbox')) as HTMLInputElement[];
    expect(boxes.filter((b) => b.checked)).toHaveLength(2);
    expect(await violations()).toEqual([]);

    await fireEvent.click(screen.getByRole('button', { name: 'Add to the Explorer menu' }));
    await waitFor(() => expect(ipc.explorerEnable).toHaveBeenCalledOnce());
    const [items, openTitle] = ipc.explorerEnable.mock.calls[0] as [{ tool: string; title: string }[], string];
    expect(items.map((i) => i.tool)).toEqual(['compress', 'merge']);
    expect(items.every((i) => i.title.length > 0)).toBe(true);
    expect(openTitle).toBe('Open with Quirewise');
    await screen.findByText('The Explorer menu is on.');
    expect(screen.getByRole('button', { name: 'Remove from the Explorer menu' })).toBeTruthy();
  });

  it('stops at five tools', async () => {
    render(Explorer);
    const boxes = (await screen.findAllByRole('checkbox')) as HTMLInputElement[];
    for (const box of boxes.filter((b) => !b.checked).slice(0, 3)) await fireEvent.click(box);
    const unchecked = (screen.getAllByRole('checkbox') as HTMLInputElement[]).filter((b) => !b.checked);
    expect(unchecked.length).toBeGreaterThan(0);
    expect(unchecked.every((b) => b.disabled)).toBe(true);
  });

  it('removes the menu', async () => {
    ipc.explorerStatus.mockResolvedValue(true);
    ipc.explorerDisable.mockResolvedValue(undefined);
    render(Explorer);
    await screen.findByText('The Explorer menu is on.');
    await fireEvent.click(screen.getByRole('button', { name: 'Remove from the Explorer menu' }));
    await waitFor(() => expect(ipc.explorerDisable).toHaveBeenCalledOnce());
    await screen.findByText('The Explorer menu is off.');
  });
});
