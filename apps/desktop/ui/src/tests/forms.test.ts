import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import {
  analysis,
  byTextSchema,
  editMetadataSchema,
  everyNSchema,
  flattenSchema,
  mergeSchema,
  organizeSchema,
  protectSchema,
  removeAnnotationsSchema,
  report,
  rotateSchema,
} from './fixtures';

const ipc = vi.hoisted(() => ({
  appVersion: vi.fn(),
  listTools: vi.fn(),
  classifyInputs: vi.fn(),
  inspectDocument: vi.fn(),
  renderThumbnail: vi.fn(),
  recentList: vi.fn(),
  recentAdd: vi.fn(),
  runBatch: vi.fn(),
  getSettings: vi.fn(),
}));

vi.mock('../lib/ipc/client', () => ({
  ipc,
  toAppError: (e: unknown) => e,
  onBatchEvent: () => Promise.resolve(() => undefined),
}));
vi.mock('../lib/dnd', () => ({ listenForDrops: () => Promise.reject(new Error('no webview')) }));

import App from '../app/App.svelte';
import { files } from '../lib/stores/files.svelte';
import { jobs } from '../lib/stores/jobs.svelte';
import { toolOptions } from '../lib/stores/options.svelte';
import { tools } from '../lib/stores/tools.svelte';
import { organizer } from '../lib/features/organize/organizer.svelte';

const list = [
  { id: 'merge', group: 'organize', label: 'merged', input_mode: 'all-together', options_schema: mergeSchema },
  { id: 'split-every-n', group: 'organize', label: 'part', input_mode: 'per-file', options_schema: everyNSchema },
  { id: 'rotate', group: 'organize', label: 'rotated', input_mode: 'per-file', options_schema: rotateSchema },
  { id: 'organize', group: 'organize', label: 'organized', input_mode: 'all-together', options_schema: organizeSchema },
  { id: 'split-by-text', group: 'organize', label: 'part', input_mode: 'per-file', options_schema: byTextSchema },
  { id: 'protect', group: 'security', label: 'protected', input_mode: 'per-file', options_schema: protectSchema },
  { id: 'edit-metadata', group: 'document', label: 'edited', input_mode: 'per-file', options_schema: editMetadataSchema },
  {
    id: 'remove-annotations',
    group: 'document',
    label: 'clean',
    input_mode: 'per-file',
    options_schema: removeAnnotationsSchema,
  },
  { id: 'flatten', group: 'document', label: 'flat', input_mode: 'per-file', options_schema: flattenSchema },
];

beforeEach(() => {
  files.clear();
  tools.select(null);
  vi.resetAllMocks();
  ipc.appVersion.mockResolvedValue('0.1.0');
  ipc.listTools.mockResolvedValue(list);
  ipc.recentList.mockResolvedValue([]);
  ipc.renderThumbnail.mockResolvedValue({ width: 10, height: 14, page_count: 2, png: 'iVBORw0KGgo=' });
  ipc.classifyInputs.mockImplementation((paths: string[]) => Promise.resolve(analysis(paths)));
  ipc.inspectDocument.mockResolvedValue(report());
  ipc.recentAdd.mockResolvedValue(undefined);
  ipc.getSettings.mockResolvedValue({ theme: 'system', remember_recent_files: true });
  ipc.runBatch.mockResolvedValue(1);
  jobs.running = false;
  jobs.status = null;
});

async function violations(): Promise<string[]> {
  const result = await axe.run(document.body, {
    rules: { 'color-contrast': { enabled: false }, region: { enabled: false } },
  });
  return result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html.slice(0, 80)).join(' | ')}`);
}

async function open(toolName: string, paths: string[]): Promise<void> {
  render(App);
  await screen.findByRole('button', { name: toolName });
  await files.add(paths);
  await fireEvent.click(screen.getByRole('button', { name: toolName }));
}

describe('option forms generated from the schema', () => {
  it('shows the fields of a tool with labels, defaults and bounds', async () => {
    await open('Split every N pages', ['C:\\a.pdf']);
    const input = (await screen.findByLabelText('Pages per file')) as HTMLInputElement;
    expect(input.value).toBe('1');
    expect(input.min).toBe('1');
    expect(await violations()).toEqual([]);
  });

  it('sends what the user chose, and defaults for the rest', async () => {
    await open('Rotate', ['C:\\a.pdf']);
    await fireEvent.click(await screen.findByLabelText('180°'));
    await fireEvent.input(screen.getByLabelText('Pages to rotate'), { target: { value: 'even' } });
    await fireEvent.click(screen.getByRole('button', { name: /Run Rotate on 1 file/ }));
    await waitFor(() => expect(ipc.runBatch).toHaveBeenCalled());
    expect(ipc.runBatch.mock.calls[0]?.[0].options).toEqual({ tool: 'rotate', angle: '180', pages: 'even' });
  });

  it('keeps a tool\'s choices when switching away and back, and resets on request', async () => {
    await open('Split every N pages', ['C:\\a.pdf']);
    await fireEvent.input(await screen.findByLabelText('Pages per file'), { target: { value: '4' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Rotate' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Split every N pages' }));
    expect((await screen.findByLabelText('Pages per file') as HTMLInputElement).value).toBe('4');
    await fireEvent.click(screen.getByRole('button', { name: 'Reset to defaults' }));
    await waitFor(() => expect((screen.getByLabelText('Pages per file') as HTMLInputElement).value).toBe('1'));
  });

  it('shows one group of settings per file and follows the files when they are reordered', async () => {
    await open('Merge PDF', ['C:\\a.pdf', 'C:\\b.pdf']);
    expect(await screen.findByRole('group', { name: 'a.pdf' })).toBeTruthy();
    const second = screen.getByRole('group', { name: 'b.pdf' });
    const pages = second.querySelector('input[type="text"]') as HTMLInputElement;
    await fireEvent.input(pages, { target: { value: '1' } });
    expect(await violations()).toEqual([]);

    const merge = tools.items.find((t) => t.id === 'merge');
    if (!merge) throw new Error('merge missing');
    const [a, b] = files.entries;
    files.move(a?.id ?? 0, 1);
    const options = toolOptions.request(merge, files.entries.map((e) => e.id)) as unknown as {
      per_file: { pages: string }[];
    };
    // b.pdf moved first, and its setting went with it.
    expect(files.entries[0]?.id).toBe(b?.id);
    expect(options.per_file.map((p) => p.pages)).toEqual(['1', 'all']);
  });
});

describe('forms for the optimize and security tools', () => {
  it('hides passwords while typing, groups the permissions and sends them as typed', async () => {
    await open('Protect with a password', ['C:\\a.pdf']);
    const opening = (await screen.findByLabelText('Password to open the file')) as HTMLInputElement;
    expect(opening.type).toBe('password');
    expect((screen.getByLabelText('Password to change the restrictions') as HTMLInputElement).type).toBe('password');
    const group = screen.getByRole('group', { name: 'What a reader may do' });
    expect(group.querySelectorAll('input[type="checkbox"]')).toHaveLength(6);
    expect(await violations()).toEqual([]);

    await fireEvent.input(opening, { target: { value: 'open' } });
    await fireEvent.input(screen.getByLabelText('Password to change the restrictions'), { target: { value: 'boss' } });
    await fireEvent.click(screen.getByLabelText('Copy text and images'));
    await fireEvent.click(screen.getByRole('button', { name: /Run Protect with a password on 1 file/ }));
    await waitFor(() => expect(ipc.runBatch).toHaveBeenCalled());
    expect(ipc.runBatch.mock.calls[0]?.[0].options).toMatchObject({
      tool: 'protect',
      user_password: 'open',
      owner_password: 'boss',
      cipher: 'aes256',
      permissions: { copy: false, print: true, modify: true },
    });
  });

  it('keeps a metadata field unless it is edited or removed, and hides the rest after remove all', async () => {
    await open('Edit metadata', ['C:\\a.pdf']);
    const title = (await screen.findByLabelText('Title')) as HTMLInputElement;
    expect(title.value).toBe('');
    expect(title.placeholder).toBe('Keep the current value');
    expect(screen.getByRole('radio', { name: 'Keep as it is' })).toBeTruthy();
    expect(await violations()).toEqual([]);

    await fireEvent.click(screen.getByLabelText('Remove Author'));
    expect((screen.getByLabelText('Author') as HTMLInputElement).disabled).toBe(true);
    await fireEvent.input(title, { target: { value: 'Annual report' } });
    await fireEvent.click(screen.getByRole('radio', { name: 'Unknown' }));
    await fireEvent.click(screen.getByRole('button', { name: /Run Edit metadata on 1 file/ }));
    await waitFor(() => expect(ipc.runBatch).toHaveBeenCalled());
    expect(ipc.runBatch.mock.calls[0]?.[0].options).toMatchObject({
      tool: 'edit-metadata',
      title: 'Annual report',
      author: ' ',
      trapped: 'unknown',
    });

    await fireEvent.click(screen.getByLabelText('Remove all metadata'));
    await waitFor(() => expect(screen.queryByLabelText('Title')).toBeNull());
  });

  it('lets several annotation kinds be ticked together', async () => {
    await open('Remove annotations', ['C:\\a.pdf']);
    const all = (await screen.findByLabelText('All except form fields and signatures')) as HTMLInputElement;
    expect(all.checked).toBe(true);
    await fireEvent.click(all);
    await fireEvent.click(screen.getByLabelText('Highlights'));
    await fireEvent.click(screen.getByLabelText('Links'));
    await fireEvent.click(screen.getByRole('button', { name: /Run Remove annotations on 1 file/ }));
    await waitFor(() => expect(ipc.runBatch).toHaveBeenCalled());
    expect(ipc.runBatch.mock.calls[0]?.[0].options).toMatchObject({
      tool: 'remove-annotations',
      kinds: ['highlights', 'links'],
    });
  });

  it('asks for a resolution only when everything becomes a picture', async () => {
    await open('Flatten', ['C:\\a.pdf']);
    await screen.findByLabelText('Form fields and annotations');
    expect(screen.queryByRole('group', { name: 'Picture resolution' })).toBeNull();
    await fireEvent.click(screen.getByLabelText('Everything (every page becomes a picture)'));
    expect(await screen.findByRole('group', { name: 'Picture resolution' })).toBeTruthy();
  });
});

describe('split by text region', () => {
  it('asks for a region until one is typed, then keeps it as fractions', async () => {
    await open('Split by text', ['C:\\a.pdf']);
    expect(await screen.findByText('Draw a region to run this tool.')).toBeTruthy();
    const width = screen.getByLabelText('Width') as HTMLInputElement;
    await fireEvent.change(width, { target: { value: '40' } });
    await screen.findByText('Region set.');
    const split = tools.items.find((t) => t.id === 'split-by-text');
    if (!split) throw new Error('missing');
    const request = toolOptions.request(split, [1]) as unknown as { region: { width: number } | null };
    expect(request.region?.width).toBeCloseTo(0.4);
    expect(await violations()).toEqual([]);
  });
});

describe('organizer', () => {
  it('lists every page, edits them, and sends the final list with files by position', async () => {
    await open('Organize pages', ['C:\\a.pdf', 'C:\\b.pdf']);
    await screen.findByText('4 pages, 0 selected');

    const cards = (): HTMLElement[] => screen.getAllByRole('button', { name: /^Position/ });
    await fireEvent.click(cards()[0] as HTMLElement);
    await fireEvent.click(screen.getByRole('button', { name: 'Rotate right' }));
    await fireEvent.click(cards()[3] as HTMLElement);
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await screen.findByText('3 pages, 0 selected');
    await fireEvent.click(screen.getByRole('button', { name: 'Insert blank page' }));
    await screen.findByText('4 pages, 1 selected');
    expect(await violations()).toEqual([]);

    await fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    await screen.findByText('3 pages, 0 selected');
    await fireEvent.click(screen.getByRole('button', { name: 'Redo' }));
    await screen.findByText('4 pages, 0 selected');

    await fireEvent.click(screen.getByRole('button', { name: /Run Organize pages/ }));
    await waitFor(() => expect(ipc.runBatch).toHaveBeenCalled());
    const sent = ipc.runBatch.mock.calls[0]?.[0].options.pages;
    expect(sent).toEqual([
      { kind: 'page', source: 0, page: 0, rotate: '90' },
      { kind: 'page', source: 0, page: 1, rotate: '0' },
      { kind: 'page', source: 1, page: 0, rotate: '0' },
      { kind: 'blank', width_pt: 612, height_pt: 792 },
    ]);
    expect(jobs.running).toBe(true);
  });

  it('starts over when the set of files changes', async () => {
    await open('Organize pages', ['C:\\a.pdf']);
    await screen.findByText('2 pages, 0 selected');
    await files.add(['C:\\b.pdf']);
    await screen.findByText('4 pages, 0 selected');
    expect(organizer.pages).toHaveLength(4);
  });
});

describe('file order for combining tools', () => {
  it('sorts by name in natural order and by modified time, and only offers it to tools that combine files', async () => {
    ipc.classifyInputs.mockImplementation((paths: string[]) =>
      Promise.resolve({
        ...analysis(paths),
        files: analysis(paths).files.map((f, i) => ({ ...f, modified: [10, 30, null][i] ?? null })),
      }),
    );
    await open('Rotate', ['C:\\part 10.pdf', 'C:\\Part 2.pdf', 'C:\\undated.pdf']);
    await waitFor(() => expect(files.entries).toHaveLength(3));
    expect(screen.queryByRole('button', { name: 'Sort by name' })).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: 'Merge PDF' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Sort by name' }));
    expect(files.entries.map((e) => e.name)).toEqual(['Part 2.pdf', 'part 10.pdf', 'undated.pdf']);

    await fireEvent.click(screen.getByRole('button', { name: 'Sort by date' }));
    // Oldest first, and the file without a date goes last.
    expect(files.entries.map((e) => e.name)).toEqual(['part 10.pdf', 'Part 2.pdf', 'undated.pdf']);
  });
});
