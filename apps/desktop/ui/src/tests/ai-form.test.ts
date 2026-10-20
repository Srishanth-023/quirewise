import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';

const ipc = vi.hoisted(() => ({
  aiCatalog: vi.fn(),
  aiModels: vi.fn(),
  componentsInstalled: vi.fn(),
}));

vi.mock('../lib/ipc/client', () => ({
  ipc,
  toAppError: (e: unknown) => e,
  onComponentProgress: vi.fn(),
}));

import AiPicker from '../lib/forms/AiPicker.svelte';
import LanguageChoice from '../lib/forms/LanguageChoice.svelte';
import { formFor, ruleMet } from '../lib/forms/toolForms';
import { catalog } from '../lib/features/ai/catalog.svelte';
import { components } from '../lib/features/components/store.svelte';

const models = [
  {
    id: 'qwen2.5-1.5b-instruct',
    name: 'Qwen2.5 1.5B Instruct',
    license: 'Apache-2.0',
    license_url: 'https://www.apache.org/licenses/LICENSE-2.0',
    license_summary: 'Free to use and change.',
    file: 'q.gguf',
    size_mb: 1120,
    note: 'The default.',
  },
  {
    id: 'qwen2.5-7b-instruct',
    name: 'Qwen2.5 7B Instruct',
    license: 'Apache-2.0',
    license_url: 'https://www.apache.org/licenses/LICENSE-2.0',
    license_summary: 'Free to use and change.',
    file: 'q7.gguf',
    size_mb: 4680,
    note: 'Better.',
  },
];

const languages = [
  { code: 'en', name: 'English', drawable: true },
  { code: 'de', name: 'German', drawable: true },
  { code: 'ar', name: 'Arabic', drawable: false },
];

const installed = (...ids: string[]) =>
  ids.map((id) => ({ id, version: '1', arch: 'x64', url: 'https://x.y/z', size: 1, sha256: '0'.repeat(64) }));

beforeEach(() => {
  ipc.aiCatalog.mockReset().mockResolvedValue({
    models,
    languages,
    default_model: 'qwen2.5-1.5b-instruct',
    default_ollama_url: 'http://127.0.0.1:11434',
  });
  ipc.aiModels.mockReset();
  ipc.componentsInstalled.mockReset().mockResolvedValue([]);
  catalog.data = null;
  components.installed = [];
});

describe('the model picker', () => {
  it('shows the default model with its license and says when it is not downloaded', async () => {
    render(AiPicker, { id: 'ai', label: 'Language model', value: {}, onchange: vi.fn() });
    expect(await screen.findByText(/License: Apache-2.0\./)).toBeTruthy();
    expect(screen.getByText(/Free to use and change\./)).toBeTruthy();
    expect(screen.getByText(/Not downloaded yet/)).toBeTruthy();
    const select = screen.getByLabelText('Model') as HTMLSelectElement;
    expect(select.value).toBe('qwen2.5-1.5b-instruct');
    expect(screen.getByRole('option', { name: 'Qwen2.5 7B Instruct, 4.7 GB' })).toBeTruthy();
  });

  it('is ready when the program and the model are installed', async () => {
    ipc.componentsInstalled.mockResolvedValue(installed('llama-cpp', 'model-qwen2.5-1.5b-instruct'));
    render(AiPicker, { id: 'ai', label: 'Language model', value: {}, onchange: vi.fn() });
    expect(await screen.findByText('Ready to use.')).toBeTruthy();
  });

  it('needs the program as well as the model', async () => {
    ipc.componentsInstalled.mockResolvedValue(installed('model-qwen2.5-1.5b-instruct'));
    render(AiPicker, { id: 'ai', label: 'Language model', value: {}, onchange: vi.fn() });
    expect(await screen.findByText(/Not downloaded yet/)).toBeTruthy();
  });

  it('reports the model that is chosen and keeps the other values', async () => {
    const onchange = vi.fn();
    render(AiPicker, { id: 'ai', label: 'Language model', value: { backend: 'bundled', ollama_url: 'x' }, onchange });
    await screen.findByRole('option', { name: 'Qwen2.5 7B Instruct, 4.7 GB' });
    const select = screen.getByLabelText('Model');
    await fireEvent.change(select, { target: { value: 'qwen2.5-7b-instruct' } });
    expect(onchange).toHaveBeenCalledWith({ backend: 'bundled', ollama_url: 'x', model: 'qwen2.5-7b-instruct' });
  });

  it('asks nothing of Ollama until the person agrees', async () => {
    const onchange = vi.fn();
    render(AiPicker, { id: 'ai', label: 'Language model', value: { backend: 'ollama' }, onchange });
    await screen.findByText(/Only addresses on this computer are accepted/);
    expect(screen.queryByText('Find the models')).toBeNull();
    expect(ipc.aiModels).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByLabelText(/Allow Quirewise to talk to the Ollama server/));
    expect(onchange).toHaveBeenCalledWith({ backend: 'ollama', allow_ollama: true });
  });

  it('lists the models of the server after the agreement', async () => {
    ipc.aiModels.mockResolvedValue(['llama3.2:3b', 'mistral:7b']);
    const onchange = vi.fn();
    render(AiPicker, {
      id: 'ai',
      label: 'Language model',
      value: { backend: 'ollama', allow_ollama: true },
      onchange,
    });
    await fireEvent.click(await screen.findByText('Find the models'));
    await waitFor(() => expect(ipc.aiModels).toHaveBeenCalledTimes(1));
    expect(ipc.aiModels.mock.calls[0]?.[0]).toMatchObject({ backend: 'ollama', allow_ollama: true });
    const select = (await screen.findByRole('option', { name: 'mistral:7b' })).parentElement as HTMLSelectElement;
    await fireEvent.change(select, { target: { value: 'mistral:7b' } });
    expect(onchange).toHaveBeenLastCalledWith({ backend: 'ollama', allow_ollama: true, model: 'mistral:7b' });
  });

  it('forgets the model when the backend changes, since the names differ', async () => {
    const onchange = vi.fn();
    render(AiPicker, {
      id: 'ai',
      label: 'Language model',
      value: { backend: 'bundled', model: 'qwen2.5-7b-instruct' },
      onchange,
    });
    await fireEvent.click(await screen.findByLabelText('Ollama on this computer'));
    expect(onchange).toHaveBeenCalledWith({ backend: 'ollama', model: null });
  });
});

describe('the language choice', () => {
  it('offers every language for Markdown and leaves out what a PDF cannot show', async () => {
    const { unmount } = render(LanguageChoice, {
      id: 'l',
      label: 'Language',
      value: 'en',
      drawableOnly: false,
      onchange: vi.fn(),
    });
    await screen.findByRole('option', { name: /Arabic/ });
    unmount();
    render(LanguageChoice, { id: 'l', label: 'Language', value: 'en', drawableOnly: true, onchange: vi.fn() });
    await screen.findByRole('option', { name: 'German' });
    expect(screen.queryByRole('option', { name: /Arabic/ })).toBeNull();
  });

  it('reports the choice', async () => {
    const onchange = vi.fn();
    render(LanguageChoice, { id: 'l', label: 'Language', value: 'en', drawableOnly: true, onchange });
    await screen.findByRole('option', { name: 'German' });
    const select = screen.getByLabelText('Language');
    await fireEvent.change(select, { target: { value: 'de' } });
    expect(onchange).toHaveBeenCalledWith('de');
  });
});

describe('the forms of the AI tools', () => {
  it('draw the model as a picker and the language as a choice', () => {
    for (const id of ['smart-split', 'ai-summarizer', 'translate-pdf', 'redact', 'pdf-to-markdown']) {
      expect(formFor(id).aiPicker, id).toEqual(['ai']);
    }
    expect(formFor('ai-summarizer').languageChoice).toEqual(['language']);
    expect(formFor('translate-pdf').languageChoice).toEqual(['target_language']);
    expect(formFor('smart-split').smartSplit).toBe(true);
    expect(formFor('smart-split').custom).toEqual(['parts']);
  });

  it('show the model only where the AI option is on', () => {
    expect(ruleMet(formFor('redact').visibleWhen?.ai, { ai_suggestions: false })).toBe(false);
    expect(ruleMet(formFor('redact').visibleWhen?.ai, { ai_suggestions: true })).toBe(true);
    expect(ruleMet(formFor('pdf-to-markdown').visibleWhen?.ai, { ai_cleanup: false })).toBe(false);
    expect(ruleMet(formFor('pdf-to-markdown').visibleWhen?.ai, { ai_cleanup: true })).toBe(true);
    // The tools that are the AI always show it.
    expect(formFor('ai-summarizer').visibleWhen?.ai).toBeUndefined();
  });
});
