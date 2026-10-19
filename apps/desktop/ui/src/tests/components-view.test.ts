import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';

const ipc = vi.hoisted(() => ({
  componentsInstalled: vi.fn(),
  componentsAvailable: vi.fn(),
  componentInstall: vi.fn(),
  componentImport: vi.fn(),
  componentRemove: vi.fn(),
  pickPaths: vi.fn(),
}));
const progress = vi.hoisted(() => ({ listener: undefined as ((p: unknown) => void) | undefined }));

vi.mock('../lib/ipc/client', () => ({
  ipc,
  toAppError: (e: unknown) => e,
  onComponentProgress: (handler: (p: unknown) => void) => {
    progress.listener = handler;
    return Promise.resolve(() => undefined);
  },
}));

import View from '../lib/features/components/View.svelte';
import { components } from '../lib/features/components/store.svelte';

const office = {
  id: 'libreoffice',
  version: '25.2.1',
  arch: 'x64',
  url: 'https://components.invalid/lo.zip',
  size: 330 * 1024 * 1024,
  sha256: 'a'.repeat(64),
};

beforeEach(() => {
  vi.resetAllMocks();
  components.installed = [];
  components.available = null;
  components.consent = null;
  components.busy = null;
  ipc.componentsInstalled.mockResolvedValue([]);
});

describe('components', () => {
  it('asks before the internet is used and lists what the server offers after a yes', async () => {
    ipc.componentsAvailable.mockResolvedValue([office]);
    render(View);
    await fireEvent.click(await screen.findByRole('button', { name: 'Check for components' }));
    expect(ipc.componentsAvailable).not.toHaveBeenCalled();
    expect(await screen.findByRole('dialog')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    await waitFor(() => expect(ipc.componentsAvailable).toHaveBeenCalledWith(true));
    expect(await screen.findByText('LibreOffice')).toBeTruthy();
    expect(screen.getByText('330 MB')).toBeTruthy();
  });

  it('does not contact the server when the question is declined', async () => {
    render(View);
    await fireEvent.click(await screen.findByRole('button', { name: 'Check for components' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(ipc.componentsAvailable).not.toHaveBeenCalled();
  });

  it('names the download before installing, shows its progress and lists it afterwards', async () => {
    ipc.componentsAvailable.mockResolvedValue([office]);
    let finish: (value: unknown) => void = () => undefined;
    ipc.componentInstall.mockImplementation(() => new Promise((resolve) => (finish = resolve)));
    render(View);
    await fireEvent.click(await screen.findByRole('button', { name: 'Check for components' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Continue' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Install LibreOffice' }));
    expect(await screen.findByText(/download LibreOffice 25.2.1 \(330 MB\)/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    await waitFor(() => expect(ipc.componentInstall).toHaveBeenCalledWith('libreoffice', true));
    await waitFor(() => expect(progress.listener).toBeDefined());
    progress.listener?.({ id: 'libreoffice', done: 110 * 1024 * 1024, total: 330 * 1024 * 1024 });
    const bar = await screen.findByRole('progressbar', { name: 'Installing LibreOffice' });
    expect(bar.getAttribute('aria-valuenow')).toBe(String(110 * 1024 * 1024));
    ipc.componentsInstalled.mockResolvedValue([office]);
    finish(office);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Remove LibreOffice' })).toBeTruthy());
    expect(await axe.run(document.body, { rules: { 'color-contrast': { enabled: false }, region: { enabled: false } } }).then((r) => r.violations.map((v) => v.id))).toEqual([]);
  });

  it('installs from a package file without any question and removes a component', async () => {
    ipc.pickPaths.mockResolvedValue(['C:\\lo.qwpkg']);
    ipc.componentImport.mockResolvedValue(office);
    render(View);
    await fireEvent.click(await screen.findByRole('button', { name: 'Install from a package file...' }));
    await waitFor(() => expect(ipc.componentImport).toHaveBeenCalledWith('C:\\lo.qwpkg'));
    expect(ipc.pickPaths).toHaveBeenCalledWith('package', false);

    ipc.componentsInstalled.mockResolvedValue([office]);
    await components.load();
    ipc.componentRemove.mockResolvedValue(undefined);
    ipc.componentsInstalled.mockResolvedValue([]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Remove LibreOffice' }));
    await waitFor(() => expect(ipc.componentRemove).toHaveBeenCalledWith('libreoffice'));
  });
});
