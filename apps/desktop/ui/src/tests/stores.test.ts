import { beforeEach, describe, expect, it, vi } from 'vitest';
import { analysis, report } from './fixtures';

const ipc = vi.hoisted(() => ({
  classifyInputs: vi.fn(),
  inspectDocument: vi.fn(),
  recentAdd: vi.fn(),
  runBatch: vi.fn(),
  cancelJob: vi.fn(),
  retryFailed: vi.fn(),
  listTools: vi.fn(),
}));
const handlers = vi.hoisted(() => ({ batch: undefined as undefined | ((e: unknown) => void) }));

vi.mock('../lib/ipc/client', () => ({
  ipc,
  toAppError: (e: unknown) =>
    typeof e === 'object' && e !== null && 'code' in e
      ? e
      : { code: 'QW-INTERNAL', message: 'Something went wrong.' },
  onBatchEvent: (h: (e: unknown) => void) => {
    handlers.batch = h;
    return Promise.resolve(() => undefined);
  },
}));

import { files } from '../lib/stores/files.svelte';
import { jobs } from '../lib/stores/jobs.svelte';
import { tools } from '../lib/stores/tools.svelte';
import type { ToolDescriptor } from '../lib/ipc/generated';

const wrongPassword = { code: 'QW-PDF-WRONG-PASSWORD', message: 'Wrong password.' };
const encrypted = { code: 'QW-PDF-ENCRYPTED', message: 'Locked.' };

beforeEach(() => {
  files.clear();
  vi.resetAllMocks();
  ipc.recentAdd.mockResolvedValue(undefined);
  ipc.classifyInputs.mockImplementation((paths: string[]) => Promise.resolve(analysis(paths)));
  ipc.inspectDocument.mockResolvedValue(report());
});

describe('file set', () => {
  it('classifies, adds and inspects PDFs, ignoring duplicates', async () => {
    await files.add(['C:\\a.pdf', 'C:\\b.pdf']);
    await files.add(['C:\\a.pdf']);
    expect(files.entries.map((e) => e.name)).toEqual(['a.pdf', 'b.pdf']);
    expect(files.entries[0]?.report?.page_count).toBe(2);
    expect(files.selected?.name).toBe('a.pdf');
    expect(files.suggestions).toContain('merge');
    expect(ipc.recentAdd).toHaveBeenCalledTimes(2);
  });

  it('skips unusable files with a reason and keeps the rest', async () => {
    ipc.classifyInputs.mockResolvedValue({
      files: [
        {
          path: 'C:\\x.zip',
          name: 'x.zip',
          kind: null,
          error: { code: 'QW-INPUT-NOT-PDF', message: 'Not a PDF.' },
        },
        { path: 'C:\\a.pdf', name: 'a.pdf', kind: 'pdf', error: null },
      ],
      suggestions: [],
    });
    await files.add(['C:\\x.zip', 'C:\\a.pdf']);
    expect(files.entries.map((e) => e.name)).toEqual(['a.pdf']);
  });

  it('keeps a password only in memory and forgets a wrong one', async () => {
    ipc.inspectDocument.mockRejectedValueOnce(encrypted);
    await files.add(['C:\\locked.pdf']);
    const id = files.entries[0]?.id ?? -1;
    expect(files.entries[0]?.problem?.code).toBe('QW-PDF-ENCRYPTED');

    ipc.inspectDocument.mockRejectedValueOnce(wrongPassword);
    await files.unlock(id, 'nope');
    expect(files.entries[0]?.problem?.code).toBe('QW-PDF-WRONG-PASSWORD');
    expect(files.passwordFor(id)).toBeNull();

    await files.unlock(id, 'open');
    expect(ipc.inspectDocument).toHaveBeenLastCalledWith('C:\\locked.pdf', 'open');
    expect(files.passwordFor(id)).toBe('open');
    expect(files.entries[0]?.problem).toBeUndefined();
    expect(files.entries[0]?.report?.locked).toBe(false);
  });

  it('reorders and removes files, moving the selection', async () => {
    await files.add(['C:\\a.pdf', 'C:\\b.pdf', 'C:\\c.pdf']);
    const [a, b, c] = files.entries.map((e) => e.id) as [number, number, number];
    files.move(c, -1);
    expect(files.entries.map((e) => e.name)).toEqual(['a.pdf', 'c.pdf', 'b.pdf']);
    files.move(a, -1);
    expect(files.entries[0]?.name).toBe('a.pdf');
    files.select(b);
    files.remove(b);
    expect(files.selected?.name).toBe('a.pdf');
    expect(files.entries).toHaveLength(2);
  });
});

const rotate: ToolDescriptor = {
  id: 'rotate',
  group: 'edit',
  label: 'rotated',
  input_mode: 'per-file',
  options_schema: {},
};

describe('tools', () => {
  it('searches names and groups in registry order', async () => {
    ipc.listTools.mockResolvedValue([
      rotate,
      { ...rotate, id: 'merge' as never, group: 'organize', label: 'merged' },
    ]);
    await tools.load();
    expect(tools.search('rot').map((t) => t.id)).toEqual(['rotate']);
    expect(tools.grouped().map((g) => g.group)).toEqual(['edit', 'organize']);
    tools.select('rotate');
    expect(tools.selected?.id).toBe('rotate');
  });
});

describe('jobs', () => {
  it('maps batch events onto files and finishes with results', async () => {
    await files.add(['C:\\a.pdf', 'C:\\b.pdf']);
    ipc.runBatch.mockResolvedValue(7);
    await jobs.listen();
    await jobs.run(rotate);
    expect(jobs.running).toBe(true);
    expect(ipc.runBatch.mock.calls[0]?.[0].inputs).toHaveLength(2);

    const send = handlers.batch;
    expect(send).toBeDefined();
    send?.({ type: 'progress', job: 7, unit: 1, done: 1, total: 4 });
    expect(files.entries[1]?.status).toBe('running');
    expect(files.entries[1]?.progress).toEqual({ done: 1, total: 4 });

    send?.({
      type: 'unit-finished',
      job: 7,
      unit: 0,
      status: 'done',
      error: null,
      outputs: [{ path: 'C:\\a (rotated).pdf', pages: 2, bytes: 10 }],
    });
    send?.({
      type: 'unit-finished',
      job: 7,
      unit: 1,
      status: 'damaged',
      error: { code: 'QW-PDF-DAMAGED', message: 'Damaged.' },
      outputs: [],
    });
    send?.({ type: 'job-finished', job: 7, status: 'completed-with-errors' });

    expect(jobs.running).toBe(false);
    expect(jobs.status).toBe('completed-with-errors');
    expect(jobs.results.map((r) => r.path)).toEqual(['C:\\a (rotated).pdf']);
    expect(files.entries.map((e) => e.status)).toEqual(['done', 'damaged']);
    expect(jobs.hasFailures).toBe(true);
    send?.({ type: 'progress', job: 99, unit: 0, done: 1, total: 1 });
    expect(files.entries[0]?.status).toBe('done');
  });

  it('maps a merge-style job onto every file', async () => {
    await files.add(['C:\\a.pdf', 'C:\\b.pdf']);
    ipc.runBatch.mockResolvedValue(8);
    await jobs.listen();
    await jobs.run({ ...rotate, input_mode: 'all-together' });
    handlers.batch?.({
      type: 'unit-finished',
      job: 8,
      unit: 0,
      status: 'done',
      error: null,
      outputs: [],
    });
    expect(files.entries.map((e) => e.status)).toEqual(['done', 'done']);
  });
});
