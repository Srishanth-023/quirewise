<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n';
  import { ipc, toAppError, type AiOptions } from '../ipc/client';
  import { catalog } from '../features/ai/catalog.svelte';
  import { modelInstalled, serverInstalled, sizeText } from '../features/ai/ai';
  import { components } from '../features/components/store.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import type { Values } from './schema';

  interface Props {
    id: string;
    label: string;
    /** The values of the `ai` group of the tool. */
    value: Values;
    onchange: (value: Values) => void;
  }

  let { id, label, value, onchange }: Props = $props();

  onMount(() => {
    void catalog.load();
    void components.load();
  });

  const options = $derived(value as AiOptions);
  const backend = $derived(options.backend ?? 'bundled');
  const models = $derived(catalog.data?.models ?? []);
  const chosenId = $derived(options.model ?? catalog.data?.default_model ?? '');
  const chosen = $derived(models.find((m) => m.id === chosenId));
  const ready = $derived(
    chosen !== undefined &&
      serverInstalled(components.installed) &&
      modelInstalled(chosen.id, components.installed),
  );
  const allowed = $derived(options.allow_ollama === true);
  const ollamaUrl = $derived(options.ollama_url ?? catalog.data?.default_ollama_url ?? '');

  /** Models the Ollama server listed, or null until the person asked. */
  let found = $state<string[] | null>(null);
  let finding = $state(false);

  function set(patch: Partial<AiOptions>): void {
    onchange({ ...value, ...patch });
  }

  function chooseBackend(next: 'bundled' | 'ollama'): void {
    found = null;
    set({ backend: next, model: null });
  }

  async function find(): Promise<void> {
    finding = true;
    try {
      found = await ipc.aiModels({ ...options, backend: 'ollama', allow_ollama: true });
      if (found.length === 0) toasts.info(t('ai.ollama.noModels'));
    } catch (error) {
      found = null;
      toasts.error(toAppError(error));
    } finally {
      finding = false;
    }
  }

  const modelLabel = (name: string, size: number): string => t('ai.model.entry', { name, size: sizeText(size) });
</script>

<fieldset class="group" aria-describedby={`${id}-hint`}>
  <legend>{label}</legend>
  <p id={`${id}-hint`} class="hint">{t('ai.hint')}</p>

  <fieldset class="field">
    <legend>{t('ai.backend')}</legend>
    <div class="radios">
      <label class="check">
        <input type="radio" name={`${id}-backend`} checked={backend === 'bundled'} onchange={() => chooseBackend('bundled')} />
        <span>{t('ai.backend.bundled')}</span>
      </label>
      <label class="check">
        <input type="radio" name={`${id}-backend`} checked={backend === 'ollama'} onchange={() => chooseBackend('ollama')} />
        <span>{t('ai.backend.ollama')}</span>
      </label>
    </div>
  </fieldset>

  {#if backend === 'bundled'}
    <div class="field">
      <label for={`${id}-model`}>{t('ai.model')}</label>
      <select
        id={`${id}-model`}
        value={chosenId}
        onchange={(e) => set({ model: e.currentTarget.value })}
      >
        {#each models as model (model.id)}
          <option value={model.id}>{modelLabel(model.name, model.size_mb)}</option>
        {/each}
      </select>
    </div>
    {#if chosen}
      <p class="hint">{chosen.note}</p>
      <p class="hint">
        {t('ai.license', { license: chosen.license })}
        {chosen.license_summary}
        <span class="mono">{chosen.license_url}</span>
      </p>
      <p class="status" role="status">{ready ? t('ai.ready') : t('ai.notInstalled')}</p>
    {/if}
  {:else}
    <label class="check">
      <input type="checkbox" checked={allowed} onchange={(e) => set({ allow_ollama: e.currentTarget.checked })} />
      <span>{t('ai.ollama.allow')}</span>
    </label>
    <p class="hint">{t('ai.ollama.hint')}</p>
    {#if allowed}
      <div class="field">
        <label for={`${id}-url`}>{t('ai.ollama.url')}</label>
        <input
          id={`${id}-url`}
          type="text"
          spellcheck="false"
          value={ollamaUrl}
          oninput={(e) => set({ ollama_url: e.currentTarget.value })}
        />
      </div>
      <button type="button" class="find" disabled={finding} onclick={() => void find()}>
        {finding ? t('ai.ollama.finding') : t('ai.ollama.find')}
      </button>
      <div class="field">
        <label for={`${id}-ollama-model`}>{t('ai.model')}</label>
        {#if found && found.length > 0}
          <select
            id={`${id}-ollama-model`}
            value={options.model ?? ''}
            onchange={(e) => set({ model: e.currentTarget.value === '' ? null : e.currentTarget.value })}
          >
            <option value="">{t('ai.ollama.first')}</option>
            {#each found as name (name)}
              <option value={name}>{name}</option>
            {/each}
          </select>
        {:else}
          <input
            id={`${id}-ollama-model`}
            type="text"
            spellcheck="false"
            placeholder={t('ai.ollama.first')}
            value={options.model ?? ''}
            oninput={(e) => set({ model: e.currentTarget.value.trim() === '' ? null : e.currentTarget.value.trim() })}
          />
        {/if}
      </div>
    {/if}
  {/if}
</fieldset>

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: var(--space-3) var(--space-4) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    min-width: 0;
  }
  legend {
    padding: 0 var(--space-2);
    font-weight: 500;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  .field legend {
    padding: 0;
  }
  label {
    font-weight: 500;
  }
  .radios {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-5);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 28px;
    font-weight: 400;
  }
  input[type='text'],
  select {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
  }
  input[type='checkbox'],
  input[type='radio'] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
  }
  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }
  .status {
    margin: 0;
    font-weight: 500;
  }
  .find {
    align-self: flex-start;
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  .find:disabled {
    color: var(--text-muted);
  }
  .mono {
    font-family: var(--font-mono);
  }
</style>
