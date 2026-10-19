<script lang="ts">
  import { onMount } from 'svelte';
  import { t, tDynamic } from '../i18n';
  import { ipc, toAppError } from '../ipc/client';
  import { toasts } from '../stores/toasts.svelte';

  interface Props {
    id: string;
    label: string;
    /** The language codes that are chosen. */
    value: string[];
    onchange: (value: string[]) => void;
  }

  let { id, label, value, onchange }: Props = $props();

  /** Installed languages, or null until they are known. */
  let installed = $state<string[] | null>(null);

  onMount(async () => {
    try {
      installed = await ipc.ocrLanguages();
      // A language that is not installed cannot be read, so it is dropped from the choice.
      const usable = value.filter((code) => installed?.includes(code));
      if (usable.length !== value.length || (usable.length === 0 && installed.length > 0)) {
        onchange(usable.length > 0 ? usable : installed.includes('eng') ? ['eng'] : [installed[0] ?? 'eng']);
      }
    } catch (error) {
      installed = [];
      toasts.error(toAppError(error));
    }
  });

  const name = (code: string): string => tDynamic(`language.${code}`, code);

  function toggle(code: string, on: boolean): void {
    const next = on ? [...value, code] : value.filter((c) => c !== code);
    onchange(next);
  }
</script>

<fieldset class="field" aria-describedby={`${id}-hint`}>
  <legend>{label}</legend>
  {#if installed === null}
    <p class="hint">{t('languages.loading')}</p>
  {:else if installed.length === 0}
    <p class="hint" id={`${id}-hint`}>{t('languages.none')}</p>
  {:else}
    <div class="choices">
      {#each installed as code (code)}
        <label class="check">
          <input
            type="checkbox"
            checked={value.includes(code)}
            disabled={value.length === 1 && value.includes(code)}
            onchange={(e) => toggle(code, e.currentTarget.checked)}
          />
          <span>{name(code)} <span class="code">({code})</span></span>
        </label>
      {/each}
    </div>
    <p class="hint" id={`${id}-hint`}>{t('languages.hint')}</p>
  {/if}
</fieldset>

<style>
  .field {
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  legend {
    padding: 0;
    margin-bottom: var(--space-2);
    font-weight: 500;
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--target-min);
  }
  .code {
    color: var(--text-muted);
    font-family: var(--font-mono);
  }
  .hint {
    margin: var(--space-2) 0 0;
    color: var(--text-muted);
  }
</style>
