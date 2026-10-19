import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { analysis, report, rotateSchema } from './fixtures';

const ipc = vi.hoisted(() => ({
  appVersion: vi.fn(),
  listTools: vi.fn(),
  pickFiles: vi.fn(),
  classifyInputs: vi.fn(),
  inspectDocument: vi.fn(),
  renderThumbnail: vi.fn(),
  recentList: vi.fn(),
  recentAdd: vi.fn(),
  recentClear: vi.fn(),
  getSettings: vi.fn(),
  componentsInstalled: vi.fn(),
  ocrLanguages: vi.fn(),
  scanAvailable: vi.fn(),
  saveSettings: vi.fn(),
}));

vi.mock('../lib/ipc/client', () => ({
  ipc,
  toAppError: (e: unknown) => e,
  onBatchEvent: () => Promise.resolve(() => undefined),
  onComponentProgress: () => Promise.resolve(() => undefined),
}));
vi.mock('../lib/dnd', () => ({ listenForDrops: () => Promise.reject(new Error('no webview')) }));

import App from '../app/App.svelte';
import { files } from '../lib/stores/files.svelte';
import { tools } from '../lib/stores/tools.svelte';

const png = { width: 10, height: 14, page_count: 2, png: 'iVBORw0KGgo=' };

beforeEach(() => {
  files.clear();
  tools.select(null);
  vi.resetAllMocks();
  ipc.appVersion.mockResolvedValue('0.1.0');
  ipc.listTools.mockResolvedValue([
    { id: 'rotate', group: 'edit', label: 'rotated', input_mode: 'per-file', options_schema: rotateSchema },
  ]);
  ipc.recentList.mockResolvedValue([{ path: 'C:\\old.pdf', name: 'old.pdf', opened_at: 1 }]);
  ipc.renderThumbnail.mockResolvedValue(png);
  ipc.classifyInputs.mockImplementation((paths: string[]) => Promise.resolve(analysis(paths)));
  ipc.inspectDocument.mockResolvedValue(report());
  ipc.recentAdd.mockResolvedValue(undefined);
  ipc.getSettings.mockResolvedValue({ theme: 'system', remember_recent_files: true });
  ipc.componentsInstalled.mockResolvedValue([]);
  ipc.ocrLanguages.mockResolvedValue(['eng', 'deu']);
  ipc.scanAvailable.mockResolvedValue(false);
});

/** Structural accessibility rules. Color contrast needs real layout, so contrast.test.ts covers it. */
async function violations(): Promise<string[]> {
  const result = await axe.run(document.body, {
    rules: { 'color-contrast': { enabled: false }, region: { enabled: false } },
  });
  return result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html.slice(0, 80)).join(' | ')}`);
}

describe('accessibility', () => {
  it('home screen with recent files is clean', async () => {
    render(App);
    await screen.findByText('old.pdf');
    expect(await violations()).toEqual([]);
  });

  it('workspace with a file, selected tool and inspect panel is clean', async () => {
    render(App);
    await screen.findByRole('button', { name: 'Rotate' });
    await files.add(['C:\\a.pdf', 'C:\\b.pdf']);
    await fireEvent.click(screen.getByRole('button', { name: 'Rotate' }));
    await screen.findByRole('button', { name: /Run Rotate on 2 files/ });
    await waitFor(() => expect(screen.getAllByRole('img').length).toBeGreaterThan(0));
    expect(await violations()).toEqual([]);
  });

  it('a locked file shows the inline password form and stays clean', async () => {
    ipc.inspectDocument.mockRejectedValue({ code: 'QW-PDF-ENCRYPTED', message: 'Locked.' });
    render(App);
    await files.add(['C:\\locked.pdf']);
    await screen.findByLabelText(/Password for locked.pdf/);
    expect(await violations()).toEqual([]);
  });

  it('command palette and shortcut sheet are clean and keyboard reachable', async () => {
    render(App);
    await fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    await screen.findByRole('combobox');
    expect(await violations()).toEqual([]);
    await fireEvent.keyDown(window, { key: 'Escape' });
  });

  it('settings, about and the legal screens are clean', async () => {
    render(App);
    for (const name of ['Settings', 'About', 'License', 'Privacy', 'Terms']) {
      await fireEvent.click(screen.getByRole('button', { name }));
      await screen.findByRole('heading', { level: 1 });
      expect(await violations(), name).toEqual([]);
    }
  });
});
