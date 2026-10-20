<script lang="ts">
  import { onMount } from 'svelte';
  import { t, tDynamic } from '../i18n';
  import { catalog } from '../features/ai/catalog.svelte';

  interface Props {
    id: string;
    label: string;
    hint?: string;
    /** The language code that is chosen. */
    value: string;
    /** Languages a PDF cannot show are left out of the choice. */
    drawableOnly: boolean;
    onchange: (value: string) => void;
  }

  let { id, label, hint = '', value, drawableOnly, onchange }: Props = $props();

  onMount(() => void catalog.load());

  const languages = $derived(
    (catalog.data?.languages ?? []).filter((l) => !drawableOnly || l.drawable || l.code === value),
  );
</script>

<div class="field">
  <label for={id}>{label}</label>
  <select {id} {value} aria-describedby={hint ? `${id}-hint` : undefined} onchange={(e) => onchange(e.currentTarget.value)}>
    {#each languages as language (language.code)}
      <option value={language.code} disabled={drawableOnly && !language.drawable}>
        {tDynamic(`ai.language.${language.code}`, language.name)}
        {language.drawable ? '' : t('ai.language.notInPdf')}
      </option>
    {/each}
  </select>
  {#if hint}<p id={`${id}-hint`} class="hint">{hint}</p>{/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  label {
    font-weight: 500;
  }
  select {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
  }
  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
</style>
