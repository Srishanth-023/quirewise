<script lang="ts">
  import { tDynamic } from '../i18n';
  import type { Field } from './schema';

  interface Props {
    field: Field;
    value: unknown;
    toolId: string;
    /** Unique DOM id prefix. */
    id: string;
    limits?: { min?: number; max?: number };
    onchange: (value: unknown) => void;
  }

  let { field, value, toolId, id, limits, onchange }: Props = $props();

  const label = $derived(
    tDynamic(`option.${toolId}.${field.key}`, tDynamic(`option.${field.key}`, field.key.replaceAll('_', ' '))),
  );
  const hint = $derived(tDynamic(`option.${toolId}.${field.key}.hint`, tDynamic(`option.${field.key}.hint`, '')));
  const hintId = $derived(`${id}-hint`);
  // Few choices read best as a visible group; more become a menu.
  const asRadios = $derived((field.choices?.length ?? 0) <= 4);

  function choiceLabel(choice: string): string {
    return tDynamic(`choice.${field.key}.${choice}`, tDynamic(`choice.${choice}`, choice));
  }

  function parseInteger(text: string): void {
    onchange(text.trim() === '' ? Number.NaN : Number.parseInt(text, 10));
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
{:else if field.kind === 'choice' && asRadios}
  <fieldset class="field" aria-describedby={hint ? hintId : undefined}>
    <legend>{label}</legend>
    <div class="radios">
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
    <select {id} value={String(value)} onchange={(e) => onchange(e.currentTarget.value)}>
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
      aria-describedby={hint ? hintId : undefined}
      oninput={(e) => parseInteger(e.currentTarget.value)}
    />
    {#if hint}<p id={hintId} class="hint">{hint}</p>{/if}
  </div>
{:else if field.kind === 'text'}
  <div class="field">
    <label for={id}>{label}</label>
    <input
      {id}
      type="text"
      autocomplete="off"
      spellcheck="false"
      value={typeof value === 'string' ? value : ''}
      placeholder={field.nullable ? tDynamic('option.none', 'None') : undefined}
      aria-describedby={hint ? hintId : undefined}
      oninput={(e) => onchange(e.currentTarget.value)}
    />
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
  }
</style>
