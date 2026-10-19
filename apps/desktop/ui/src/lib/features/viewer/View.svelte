<script lang="ts">
  import { t } from '../../i18n';
  import PageGrid from '../../components/PageGrid/PageGrid.svelte';
  import PasswordPrompt from '../password/PasswordPrompt.svelte';
  import { files } from '../../stores/files.svelte';

  const entry = $derived(files.selected);
  const locked = $derived(
    entry?.problem?.code === 'QW-PDF-ENCRYPTED' ||
      entry?.problem?.code === 'QW-PDF-WRONG-PASSWORD' ||
      entry?.report?.locked === true,
  );
</script>

<section aria-labelledby="viewer-title">
  {#if !entry}
    <p class="empty">{t('viewer.empty')}</p>
  {:else}
    <h2 id="viewer-title">
      {entry.name}
      {#if entry.report && !entry.report.locked}
        <span class="mono count">{t('viewer.pages', { count: entry.report.page_count })}</span>
      {/if}
    </h2>
    {#if entry.kind !== 'pdf'}
      <p class="empty">{entry.kind === 'image' ? t('viewer.imageFile') : t('viewer.otherFile')}</p>
    {:else if locked}
      <PasswordPrompt {entry} />
    {:else if entry.problem}
      <p class="error">{entry.problem.message} <span class="mono">({entry.problem.code})</span></p>
    {:else if entry.report}
      {#key `${entry.id}:${files.passwordFor(entry.id) !== null}`}
        <PageGrid path={entry.path} password={files.passwordFor(entry.id)} pageCount={entry.report.page_count} />
      {/key}
    {/if}
  {/if}
</section>

<style>
  h2 {
    margin: 0 0 var(--space-4);
    overflow-wrap: anywhere;
  }
  .count {
    color: var(--text-muted);
    font-weight: 400;
  }
  .empty {
    color: var(--text-muted);
  }
  .error {
    color: var(--danger);
  }
</style>
