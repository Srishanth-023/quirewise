import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { protectSchema, rotateSchema } from './fixtures';

const ipc = vi.hoisted(() => ({
  recipesList: vi.fn(),
  recipeRoles: vi.fn(),
  recipeSave: vi.fn(),
  recipeDelete: vi.fn(),
  recipeRename: vi.fn(),
  recipeExport: vi.fn(),
  recipeImport: vi.fn(),
  pickFiles: vi.fn(),
  pickFolder: vi.fn(),
  folderFiles: vi.fn(),
  runRecipe: vi.fn(),
  cancelJob: vi.fn(),
  retryFailed: vi.fn(),
  listTools: vi.fn(),
  openOutput: vi.fn(),
  revealOutput: vi.fn(),
}));
const handlers = vi.hoisted(() => ({ batch: undefined as undefined | ((e: unknown) => void) }));

vi.mock('../lib/ipc/client', () => ({
  ipc,
  toAppError: (e: unknown) => e,
  onBatchEvent: (h: (e: unknown) => void) => {
    handlers.batch = h;
    return Promise.resolve(() => undefined);
  },
}));

import View from '../lib/features/recipes/View.svelte';
import { recipes } from '../lib/features/recipes/recipes.svelte';
import { tools } from '../lib/stores/tools.svelte';

const saved = {
  id: 'r1',
  name: 'Tidy scans',
  steps: [{ options: { tool: 'rotate', angle: '90', pages: 'all' } }],
};

beforeEach(async () => {
  vi.resetAllMocks();
  recipes.draft = null;
  recipes.chosen = null;
  recipes.paths = [];
  recipes.folder = null;
  recipes.rows = [];
  recipes.running = false;
  ipc.listTools.mockResolvedValue([
    { id: 'rotate', group: 'edit', label: 'rotated', input_mode: 'per-file', options_schema: rotateSchema },
    { id: 'protect', group: 'security', label: 'protected', input_mode: 'per-file', options_schema: protectSchema },
  ]);
  ipc.recipesList.mockResolvedValue([saved]);
  ipc.recipeRoles.mockResolvedValue([
    { tool: 'rotate', role: 'middle' },
    { tool: 'protect', role: 'last' },
  ]);
  await tools.load();
});

async function violations(): Promise<string[]> {
  const result = await axe.run(document.body, {
    rules: { 'color-contrast': { enabled: false }, region: { enabled: false } },
  });
  return result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html.slice(0, 80)).join(' | ')}`);
}

describe('recipes screen', () => {
  it('lists saved recipes with their steps and is accessible', async () => {
    render(View);
    expect(await screen.findByText('Tidy scans')).toBeTruthy();
    expect(screen.getByText(/Rotate|rotated/)).toBeTruthy();
    expect(await violations()).toEqual([]);
  });

  it('builds a recipe step by step and saves it with each tool tagged', async () => {
    ipc.recipeSave.mockImplementation((r: unknown) => Promise.resolve({ ...(r as object), id: 'r2' }));
    render(View);
    await screen.findByText('Tidy scans');
    await fireEvent.click(screen.getByRole('button', { name: /New recipe/ }));
    const save = screen.getByRole('button', { name: 'Save recipe' }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);

    await fireEvent.input(screen.getByLabelText('Recipe name'), { target: { value: 'Rotate and lock' } });
    const add = screen.getByLabelText('Add a step') as HTMLSelectElement;
    await fireEvent.change(add, { target: { value: 'rotate' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add step' }));
    await fireEvent.change(screen.getByLabelText('Add a step'), { target: { value: 'protect' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add step' }));

    expect(screen.getByText(/Step 1/)).toBeTruthy();
    expect(screen.getByText(/Step 2/)).toBeTruthy();
    // After a step that writes its own files nothing else can be added.
    expect((screen.getByLabelText('Add a step') as HTMLSelectElement).disabled).toBe(true);
    expect(await violations()).toEqual([]);

    await waitFor(() => expect((screen.getByRole('button', { name: 'Save recipe' }) as HTMLButtonElement).disabled).toBe(false));
    await fireEvent.click(screen.getByRole('button', { name: 'Save recipe' }));
    await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledOnce());
    const sent = ipc.recipeSave.mock.calls[0]?.[0] as { name: string; steps: { options: { tool: string } }[] };
    expect(sent.name).toBe('Rotate and lock');
    expect(sent.steps.map((s) => s.options.tool)).toEqual(['rotate', 'protect']);
  });

  it('explains a broken order and refuses to save it', async () => {
    render(View);
    await screen.findByText('Tidy scans');
    recipes.startNew();
    if (recipes.draft) recipes.draft.name = 'Wrong order';
    recipes.addStep('protect');
    recipes.addStep('rotate');
    await waitFor(() => expect(screen.getByRole('alert').textContent).toMatch(/Step 1 writes its own files/));
    expect((screen.getByRole('button', { name: 'Save recipe' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('runs a recipe on the chosen files and shows each file as it finishes', async () => {
    ipc.pickFiles.mockResolvedValue(['C:\\in\\a.pdf', 'C:\\in\\b.pdf']);
    ipc.runRecipe.mockResolvedValue(7);
    render(View);
    await screen.findByText('Tidy scans');
    await fireEvent.click(screen.getByRole('button', { name: 'Use' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Add files' }));
    await screen.findByText('a.pdf');
    await fireEvent.click(screen.getByRole('button', { name: /^Run Tidy scans/ }));

    await waitFor(() => expect(ipc.runRecipe).toHaveBeenCalledOnce());
    const request = ipc.runRecipe.mock.calls[0]?.[0] as { inputs: { path: string }[]; recipe: { id: string } };
    expect(request.inputs.map((i) => i.path)).toEqual(['C:\\in\\a.pdf', 'C:\\in\\b.pdf']);
    expect(request.recipe.id).toBe('r1');

    handlers.batch?.({ type: 'progress', job: 7, unit: 0, done: 500, total: 1000 });
    handlers.batch?.({
      type: 'unit-finished',
      job: 7,
      unit: 0,
      status: 'done',
      error: null,
      outputs: [{ path: 'C:\\in\\a (rotated).pdf', pages: 2, bytes: 10 }],
    });
    handlers.batch?.({
      type: 'unit-finished',
      job: 7,
      unit: 1,
      status: 'damaged',
      error: { code: 'QW-PDF-DAMAGED', message: 'Step 1 (rotate): damaged.' },
      outputs: [],
    });
    handlers.batch?.({ type: 'job-finished', job: 7, status: 'completed-with-errors' });
    // Events of other jobs are ignored.
    handlers.batch?.({ type: 'job-finished', job: 99, status: 'completed' });

    await screen.findByText(/Step 1 \(rotate\): damaged/);
    expect(screen.getByText('Saved a (rotated).pdf')).toBeTruthy();
    expect(screen.getByText('Finished: 1 file done, 1 not done.')).toBeTruthy();
    expect(recipes.hasFailures).toBe(true);
    expect(recipes.running).toBe(false);
    expect(await violations()).toEqual([]);
  });

  it('will not run with nothing to process', async () => {
    ipc.folderFiles.mockResolvedValue([]);
    render(View);
    await screen.findByText('Tidy scans');
    await fireEvent.click(screen.getByRole('button', { name: 'Use' }));
    const run = (await screen.findByRole('button', { name: /^Run Tidy scans/ })) as HTMLButtonElement;
    expect(run.disabled).toBe(true);
  });
});
