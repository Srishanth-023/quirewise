import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { analysis, report } from './fixtures';

const ipc = vi.hoisted(() => ({
  renderThumbnail: vi.fn(),
  classifyInputs: vi.fn(),
  inspectDocument: vi.fn(),
  recentAdd: vi.fn(),
  editorFonts: vi.fn(),
  formFields: vi.fn(),
  suggestFields: vi.fn(),
  textBlocks: vi.fn(),
  redactMatches: vi.fn(),
  signaturesList: vi.fn(),
  signatureSave: vi.fn(),
  signatureDelete: vi.fn(),
  pickPaths: vi.fn(),
}));

vi.mock('../lib/ipc/client', () => ({ ipc, toAppError: (e: unknown) => e }));

import EditorView from '../lib/features/editor/EditorView.svelte';
import { editor } from '../lib/features/editor/editor.svelte';
import type { ToolDescriptor } from '../lib/ipc/generated';
import { files } from '../lib/stores/files.svelte';
import { toolOptions } from '../lib/stores/options.svelte';

const tool = (id: string): ToolDescriptor => ({
  id: id as ToolDescriptor['id'],
  group: 'edit',
  label: id,
  input_mode: 'per-file',
  options_schema: { type: 'object', properties: {} },
});

const options = (id: string): Record<string, unknown[]> =>
  toolOptions.values(tool(id)) as unknown as Record<string, unknown[]>;

beforeEach(async () => {
  files.clear();
  vi.resetAllMocks();
  ipc.renderThumbnail.mockResolvedValue({ width: 10, height: 14, page_count: 2, png: 'iVBORw0KGgo=' });
  ipc.classifyInputs.mockImplementation((paths: string[]) => Promise.resolve(analysis(paths)));
  ipc.inspectDocument.mockResolvedValue(report());
  ipc.recentAdd.mockResolvedValue(undefined);
  ipc.editorFonts.mockResolvedValue(['Arial']);
  ipc.formFields.mockResolvedValue([]);
  ipc.suggestFields.mockResolvedValue([]);
  ipc.textBlocks.mockResolvedValue([]);
  ipc.redactMatches.mockResolvedValue([]);
  ipc.signaturesList.mockResolvedValue([]);
  await files.add(['C:\\a.pdf']);
  for (const id of ['edit-pdf', 'annotate', 'sign-pdf', 'fill-sign', 'create-forms', 'redact']) {
    toolOptions.reset(tool(id));
  }
});

async function violations(): Promise<string[]> {
  const result = await axe.run(document.body, {
    rules: { 'color-contrast': { enabled: false }, region: { enabled: false } },
  });
  return result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html.slice(0, 80)).join(' | ')}`);
}

const button = (name: string): HTMLElement => screen.getByRole('button', { name });

describe('the editor view', () => {
  it('places text at the center, sends it to the options, and undoes it', async () => {
    render(EditorView, { tool: tool('edit-pdf') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    expect(button('Select').getAttribute('aria-pressed')).toBe('true');

    await fireEvent.click(button('Text'));
    expect(button('Text').getAttribute('aria-pressed')).toBe('true');
    await fireEvent.click(button('Add at the center'));

    const items = options('edit-pdf').items as { kind: string; page: number }[];
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: 'text', page: 1 });
    // The new item is selected, so its details can be edited at once.
    expect(await screen.findByRole('heading', { name: 'Properties' })).toBeTruthy();

    await fireEvent.click(button('Undo'));
    expect(options('edit-pdf').items).toHaveLength(0);
    await fireEvent.click(button('Redo'));
    expect(options('edit-pdf').items).toHaveLength(1);
    expect(await violations()).toEqual([]);
  });

  it('changes the text of the selected item from its properties', async () => {
    render(EditorView, { tool: tool('edit-pdf') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    await fireEvent.click(button('Text'));
    await fireEvent.click(button('Add at the center'));
    const box = (await screen.findByLabelText('Text')) as HTMLTextAreaElement;
    await fireEvent.change(box, { target: { value: 'Hello' } });
    expect((options('edit-pdf').items as { text: string }[])[0]?.text).toBe('Hello');
  });

  it('deletes the selected item with the Delete key', async () => {
    render(EditorView, { tool: tool('edit-pdf') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    await fireEvent.click(button('Rectangle'));
    await fireEvent.click(button('Add at the center'));
    expect(options('edit-pdf').items).toHaveLength(1);
    (document.activeElement as HTMLElement | null)?.blur();
    await fireEvent.keyDown(document.body, { key: 'Delete' });
    expect(options('edit-pdf').items).toHaveLength(0);
  });

  it('adds a note for annotating', async () => {
    render(EditorView, { tool: tool('annotate') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    await fireEvent.click(button('Note'));
    await fireEvent.click(button('Add at the center'));
    expect(options('annotate').annotations).toMatchObject([{ kind: 'note', page: 1 }]);
  });

  it('types into form fields and keeps the values for the tool', async () => {
    ipc.formFields.mockResolvedValue([
      {
        name: 'first',
        kind: 'text',
        page: 1,
        multiline: false,
        options: [],
        read_only: false,
        required: false,
        value: '',
        region: { x: 0.1, y: 0.1, width: 0.4, height: 0.05 },
      },
    ]);
    render(EditorView, { tool: tool('fill-sign') });
    const field = (await screen.findByLabelText('first')) as HTMLInputElement;
    await fireEvent.change(field, { target: { value: 'Ada' } });
    expect(options('fill-sign').values).toEqual([{ name: 'first', value: 'Ada' }]);
  });

  it('says so when the file has no form fields', async () => {
    render(EditorView, { tool: tool('fill-sign') });
    expect(await screen.findByText(/no form fields/)).toBeTruthy();
  });

  it('places a typed signature', async () => {
    render(EditorView, { tool: tool('sign-pdf') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    expect(screen.getByText('Nothing chosen yet.')).toBeTruthy();
    await fireEvent.click(button('Type'));
    await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'Ada Lovelace' } });
    await fireEvent.click(button('Use this'));
    await waitFor(() => expect(editor.pending).not.toBeNull());
    expect(button('Signature').getAttribute('aria-pressed')).toBe('true');
    await fireEvent.click(button('Add at the center'));
    expect(options('sign-pdf').signatures).toMatchObject([
      { page: 1, source: { type: 'typed', text: 'Ada Lovelace' } },
    ]);
    expect(await violations()).toEqual([]);
  });

  it('finds fields and adds them all', async () => {
    ipc.suggestFields.mockResolvedValue([
      { name: 'Name', label: 'Name', kind: 'text', page: 1, region: { x: 0.2, y: 0.1, width: 0.3, height: 0.03 } },
      { name: 'Agree', label: 'Agree', kind: 'checkbox', page: 1, region: { x: 0.2, y: 0.2, width: 0.03, height: 0.02 } },
    ]);
    render(EditorView, { tool: tool('create-forms') });
    await fireEvent.click(await screen.findByRole('button', { name: 'Find fields' }));
    expect(await screen.findByText(/2 places for fields found/)).toBeTruthy();
    await fireEvent.click(button('Add all'));
    const fields = options('create-forms').fields as { name: string; kind: { type: string } }[];
    expect(fields.map((f) => f.name)).toEqual(['Name', 'Agree']);
    expect(fields.map((f) => f.kind.type)).toEqual(['text', 'checkbox']);
  });

  it('changes a block of existing text into a patch and new text', async () => {
    ipc.textBlocks.mockResolvedValue([
      { text: 'Old words', size_pt: 11, bold: false, italic: false, region: { x: 0.1, y: 0.1, width: 0.3, height: 0.03 } },
    ]);
    render(EditorView, { tool: tool('edit-pdf') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    await fireEvent.click(button('Change text'));
    await fireEvent.click(await screen.findByRole('button', { name: /Edit the text: Old words/ }));
    const items = options('edit-pdf').items as { kind: string; text?: string }[];
    expect(items.map((i) => i.kind)).toEqual(['whiteout', 'text']);
    expect(items[1]?.text).toBe('Old words');
  });

  it('draws redaction areas and sends them to the options', async () => {
    render(EditorView, { tool: tool('redact') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    await fireEvent.click(button('Area'));
    await fireEvent.click(button('Add at the center'));
    expect(options('redact').areas).toMatchObject([{ page: 1 }]);
    await fireEvent.click(button('Undo'));
    expect(options('redact').areas).toHaveLength(0);
  });

  it('shows where the words and patterns of Redact are found', async () => {
    ipc.redactMatches.mockResolvedValue([
      { page: 1, region: { x: 0.1, y: 0.1, width: 0.2, height: 0.03 } },
      { page: 2, region: { x: 0.1, y: 0.4, width: 0.2, height: 0.03 } },
    ]);
    render(EditorView, { tool: tool('redact') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    expect(screen.getByText(/No words or patterns found yet/)).toBeTruthy();
    toolOptions.set(tool('redact'), 'terms', ['secret', '  ']);
    expect(await screen.findByText(/2 places found/, undefined, { timeout: 3000 })).toBeTruthy();
    expect(ipc.redactMatches).toHaveBeenCalledTimes(1);
    const sent = ipc.redactMatches.mock.calls[0]?.[2] as { terms: string[]; pages: string };
    // Blank lines are not searched for.
    expect(sent.terms).toEqual(['secret']);
    expect(sent.pages).toBe('all');
  });

  it('starts over when another file is selected', async () => {
    render(EditorView, { tool: tool('edit-pdf') });
    await screen.findByRole('toolbar', { name: 'Editor tools' });
    await fireEvent.click(button('Text'));
    await fireEvent.click(button('Add at the center'));
    expect(editor.entries).toHaveLength(1);
    await files.add(['C:\\b.pdf']);
    files.selectedId = files.entries[1]?.id ?? null;
    await waitFor(() => expect(editor.entries).toHaveLength(0));
  });
});
