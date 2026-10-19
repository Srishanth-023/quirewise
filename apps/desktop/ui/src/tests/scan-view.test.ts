import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';

const ipc = vi.hoisted(() => ({
  scanAvailable: vi.fn(),
  scanAcquire: vi.fn(),
  saveCapture: vi.fn(),
  classifyInputs: vi.fn(),
  inspectDocument: vi.fn(),
  recentAdd: vi.fn(),
}));

vi.mock('../lib/ipc/client', () => ({
  ipc,
  toAppError: (e: unknown) => e,
}));

import View from '../lib/features/scan/View.svelte';
import type { ToolDescriptor } from '../lib/ipc/generated';
import { files } from '../lib/stores/files.svelte';
import { tools } from '../lib/stores/tools.svelte';

const scanTool: ToolDescriptor = {
  id: 'scan-to-pdf',
  group: 'convert-to-pdf',
  label: 'scan',
  input_mode: 'all-together',
  options_schema: {},
};

beforeEach(() => {
  files.clear();
  tools.select(null);
  tools.items = [scanTool];
  vi.resetAllMocks();
  ipc.scanAvailable.mockResolvedValue(true);
  ipc.classifyInputs.mockImplementation((paths: string[]) =>
    Promise.resolve({
      files: paths.map((path) => ({ path, name: path.split('\\').pop(), kind: 'image', error: null })),
      suggestions: ['images-to-pdf'],
    }),
  );
  ipc.recentAdd.mockResolvedValue(undefined);
});

describe('scanning', () => {
  it('adds the scanned picture to the list and chooses Scan to PDF', async () => {
    ipc.scanAcquire.mockResolvedValue('C:\\Docs\\Quirewise Scans\\scan-1-0.png');
    render(View);
    const button = (await screen.findByRole('button', { name: 'Scan with a scanner' })) as HTMLButtonElement;
    await waitFor(() => expect(button.disabled).toBe(false));
    await fireEvent.click(button);
    await waitFor(() => expect(files.entries).toHaveLength(1));
    await waitFor(() => expect(tools.current).toBe('scan-to-pdf'));
    expect(await axe.run(document.body, { rules: { region: { enabled: false } } }).then((r) => r.violations.map((v) => v.id))).toEqual([]);
  });

  it('adds nothing when the scanning dialog is closed', async () => {
    ipc.scanAcquire.mockResolvedValue(null);
    render(View);
    const button = (await screen.findByRole('button', { name: 'Scan with a scanner' })) as HTMLButtonElement;
    await waitFor(() => expect(button.disabled).toBe(false));
    await fireEvent.click(button);
    await waitFor(() => expect(ipc.scanAcquire).toHaveBeenCalled());
    expect(files.entries).toHaveLength(0);
  });

  it('explains when there is no scanner', async () => {
    ipc.scanAvailable.mockResolvedValue(false);
    render(View);
    expect(await screen.findByText(/No scanner was found/)).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Scan with a scanner' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows nothing when the tool is not offered', async () => {
    tools.items = [];
    render(View);
    expect(screen.queryByRole('button', { name: 'Scan with a scanner' })).toBeNull();
  });
});
