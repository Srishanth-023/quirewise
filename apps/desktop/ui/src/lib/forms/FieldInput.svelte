<script lang="ts">
  import { t, tDynamic } from '../i18n';
  import { ipc, toAppError } from '../ipc/client';
  import { toasts } from '../stores/toasts.svelte';
  import type { Field } from './schema';
  import { FALLBACK_COLOR } from './toolForms';

  interface Props {
    field: Field;
    value: unknown;
    toolId: string;
    /** Unique DOM id prefix. */
    id: string;
    limits?: { min?: number; max?: number };
    /** Text that must be hidden while typing, such as a password. */
    secret?: boolean;
    /** Offer a "remove" switch next to the text, for fields where blank means "keep". */
    clearable?: boolean;
    /** Text drawn with a color control. */
    color?: boolean;
    /** Offer a button that opens the file chooser for pictures or any file. */
    picker?: 'image' | 'files';
    onchange: (value: unknown) => void;
  }

  let {
    field,
    value,
    toolId,
    id,
    limits,
    secret = false,
    clearable = false,
    color = false,
    picker,
    onchange,
  }: Props = $props();

  const label = $derived(
    tDynamic(`option.${toolId}.${field.key}`, tDynamic(`option.${field.key}`, field.key.replaceAll('_', ' '))),
  );
  const hint = $derived(tDynamic(`option.${toolId}.${field.key}.hint`, tDynamic(`option.${field.key}.hint`, '')));
  const hintId = $derived(`${id}-hint`);
  // Few choices read best as a visible group; more become a menu.
  const asRadios = $derived((field.choices?.length ?? 0) + (field.nullable ? 1 : 0) <= 4);
  // The remove switch stores a single space: not blank, so it is sent, and the core reads a
  // blank text as "remove this field".
  const removed = $derived(clearable && value === ' ');

  function choiceLabel(choice: string): string {
    return tDynamic(
      `choice.${toolId}.${field.key}.${choice}`,
      tDynamic(`choice.${field.key}.${choice}`, tDynamic(`choice.${choice}`, choice)),
    );
  }

  function parseInteger(text: string): void {
    onchange(text.trim() === '' ? Number.NaN : Number.parseInt(text, 10));
  }

  const lines = $derived(Array.isArray(value) ? (value as string[]) : []);

  async function browse(): Promise<void> {
    try {
      const picked = await ipc.pickPaths(picker === 'image' ? 'image' : 'any', picker === 'files');
      if (picked.length === 0) return;
      if (field.kind === 'lines') {
        const known = new Set(lines);
        onchange([...lines, ...picked.filter((p) => !known.has(p))]);
      } else {
        onchange(picked[0]);
      }
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  function toggle(choice: string, on: boolean): void {
    const current = Array.isArray(value) ? (value as string[]) : [];
    onchange(on ? [...current.filter((c) => c !== choice), choice] : current.filter((c) => c !== choice));
  }
</script>

{#if field.kind === 'boolean'}
  <div class="field">
    <label class="check">
      <input
        type="checkbox"
        checked={value === true}
        aria-describedby={hint ? hintId : undefined}
        onchange={(e) => onchange(e.currentTarget.checked)}
      />
      <span>{label}</span>
    </label>
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </div>
{:else if field.kind === 'multichoice'}
  <fieldset class="field" aria-describedby={hint ? hintId : undefined}>
    <legend>{label}</legend>
    <div class="radios">
      {#each field.choices ?? [] as choice (choice.value)}
        <label class="check">
          <input
            type="checkbox"
            checked={Array.isArray(value) && value.includes(choice.value)}
            onchange={(e) => toggle(choice.value, e.currentTarget.checked)}
          />
          <span>{choiceLabel(choice.value)}</span>
        </label>
      {/each}
    </div>
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </fieldset>
{:else if field.kind === 'choice' && asRadios}
  <fieldset class="field" aria-describedby={hint ? hintId : undefined}>
    <legend>{label}</legend>
    <div class="radios">
      {#if field.nullable}
        <label class="check">
          <input type="radio" name={id} value="" checked={value == null} onchange={() => onchange(null)} />
          <span>{t('option.unchanged')}</span>
        </label>
      {/if}
      {#each field.choices ?? [] as choice (choice.value)}
        <label class="check">
          <input
            type="radio"
            name={id}
            value={choice.value}
            checked={value === choice.value}
            onchange={() => onchange(choice.value)}
          />
          <span>{choiceLabel(choice.value)}</span>
        </label>
      {/each}
    </div>
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </fieldset>
{:else if field.kind === 'choice'}
  <div class="field">
    <label for={id}>{label}</label>
    <select {id} value={value == null ? '' : String(value)} onchange={(e) => onchange(e.currentTarget.value === '' && field.nullable ? null : e.currentTarget.value)}>
      {#if field.nullable}<option value="">{t('option.unchanged')}</option>{/if}
      {#each field.choices ?? [] as choice (choice.value)}
        <option value={choice.value}>{choiceLabel(choice.value)}</option>
      {/each}
    </select>
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </div>
{:else if field.kind === 'integer'}
  <div class="field">
    <label for={id}>{label}</label>
    <input
      {id}
      type="number"
      inputmode="numeric"
      step="1"
      min={limits?.min ?? field.min}
      max={limits?.max ?? field.max}
      value={typeof value === 'number' && Number.isFinite(value) ? value : ''}
      placeholder={field.nullable ? t('option.preset') : undefined}
      aria-describedby={hint ? hintId : undefined}
      oninput={(e) => parseInteger(e.currentTarget.value)}
    />
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </div>
{:else if field.kind === 'lines'}
  <div class="field">
    <label for={id}>{label}</label>
    <textarea
      {id}
      rows="4"
      spellcheck="false"
      value={lines.join('\n')}
      aria-describedby={hint ? hintId : undefined}
      oninput={(e) => onchange(e.currentTarget.value.split(/\r?\n/))}
    ></textarea>
    {#if picker}
      <button type="button" class="browse" onclick={browse}>{t('option.browse')}</button>
    {/if}
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </div>
{:else if field.kind === 'text' && color}
  <div class="field">
    <label for={id}>{label}</label>
    <div class="row">
      <input
        {id}
        type="color"
        value={typeof value === 'string' && /^#[0-9a-fA-F]{6}$/.test(value) ? value : FALLBACK_COLOR}
        aria-describedby={hint ? hintId : undefined}
        oninput={(e) => onchange(e.currentTarget.value)}
      />
      <code>{typeof value === 'string' ? value : ''}</code>
    </div>
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </div>
{:else if field.kind === 'text'}
  <div class="field">
    <label for={id}>{label}</label>
    <div class="row">
      <input
        {id}
        type={secret ? 'password' : 'text'}
        autocomplete={secret ? 'new-password' : 'off'}
        spellcheck="false"
        disabled={removed}
        value={removed ? '' : typeof value === 'string' ? value : ''}
        placeholder={field.nullable ? t(clearable ? 'option.keep' : 'option.none') : undefined}
        aria-describedby={hint ? hintId : undefined}
        oninput={(e) => onchange(e.currentTarget.value)}
      />
      {#if picker}
        <button type="button" class="browse" onclick={browse}>{t('option.browse')}</button>
      {/if}
      {#if clearable}
        <label class="check">
          <input
            type="checkbox"
            checked={removed}
            aria-label={t('option.removeField', { field: label })}
            onchange={(e) => onchange(e.currentTarget.checked ? ' ' : '')}
          />
          <span>{t('option.remove')}</span>
        </label>
      {/if}
    </div>
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </div>
{/if}

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }
  .row input[type='text'],
  .row input[type='password'] {
    flex: 1;
  }
  label,
  legend {
    font-weight: 500;
    padding: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 28px;
    font-weight: 400;
  }
  .radios {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-5);
  }
  input[type='text'],
  input[type='password'],
  input[type='number'],
  select {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
  }
  input:disabled {
    color: var(--text-muted);
    border-color: var(--border);
  }
  input[type='checkbox'],
  input[type='radio'] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
  }
  textarea {
    padding: var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    resize: vertical;
    min-width: 0;
  }
  input[type='color'] {
    width: 48px;
    min-height: 32px;
    padding: 0;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
  }
  .browse {
    align-self: flex-start;
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
</style>
