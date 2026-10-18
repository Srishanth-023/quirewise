<script lang="ts">
  import { t, tDynamic } from '../../lib/i18n';
  import Inspect from '../../lib/features/inspect/View.svelte';
  import { files } from '../../lib/stores/files.svelte';
  import { tools, toolName } from '../../lib/stores/tools.svelte';
  import type { Suggestion } from '../../lib/ipc/generated';

  // Which registered tool answers each suggestion. A chip shows only when that tool exists, so
  // the panel never offers something this build cannot run.
  const suggestionTool: Record<Suggestion, string> = {
    unlock: 'unlock',
    ocr: 'ocr',
    merge: 'merge',
    'images-to-pdf': 'images-to-pdf',
  };

  const offered = $derived(
    files.suggestions.filter((s) => tools.items.some((tool) => tool.id === suggestionTool[s])),
  );
  const report = $derived(files.selected?.report);
</script>

<aside class="panel" aria-label={t('options.title')}>
  {#if offered.length > 0}
    <section aria-labelledby="suggest-title">
      <h2 id="suggest-title">{t('suggest.title')}</h2>
      <div class="chips">
        {#each offered as suggestion (suggestion)}
          <button type="button" onclick={() => tools.select(suggestionTool[suggestion])}>
            {tDynamic(`suggest.${suggestion}`)}
          </button>
        {/each}
      </div>
    </section>
  {/if}

  <section aria-labelledby="options-title">
    <h2 id="options-title">{t('options.title')}</h2>
    {#if tools.selected}
      <p class="muted">{toolName(tools.selected)}</p>
    {:else}
      <p class="muted">{t('options.empty')}</p>
    {/if}
  </section>

  {#if report}
    <Inspect {report} />
  {/if}
</aside>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    padding: var(--space-5);
    background: var(--surface);
    overflow: auto;
  }
  h2 {
    margin: 0 0 var(--space-3);
  }
  .muted {
    margin: 0;
    color: var(--text-muted);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .chips button {
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--accent);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--accent);
    font-weight: 500;
  }
</style>
