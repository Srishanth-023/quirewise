<script lang="ts">
  import { t, tDynamic } from '../../i18n';
  import Button from '../../components/Button/Button.svelte';
  import Icon from '../../components/Icon.svelte';
  import ProgressBar from '../../components/ProgressBar/ProgressBar.svelte';
  import { files } from '../../stores/files.svelte';
  import { jobs } from '../../stores/jobs.svelte';
  import { tools } from '../../stores/tools.svelte';

  interface Props {
    onadd: () => void;
  }

  let { onadd }: Props = $props();

  // The order of the list is the merge order, so tools that combine files offer sorting.
  const sortable = $derived(tools.selected?.input_mode === 'all-together' && files.entries.length > 1);
</script>

<section aria-labelledby="queue-title">
  <div class="head">
    <h2 id="queue-title">{t('files.title')} <span class="mono count">{t('files.count', { count: files.entries.length })}</span></h2>
    <div class="actions">
      {#if sortable}
        <Button onclick={() => files.sort('name')} disabled={jobs.running}>{t('files.sortName')}</Button>
        <Button onclick={() => files.sort('modified')} disabled={jobs.running}>{t('files.sortDate')}</Button>
      {/if}
      <Button onclick={onadd} disabled={jobs.running}>{t('files.add')}</Button>
      <Button onclick={() => files.clear()} disabled={jobs.running || files.entries.length === 0}>
        {t('files.clear')}
      </Button>
    </div>
  </div>

  <ul class="list">
    {#each files.entries as entry, index (entry.id)}
      <li class:selected={files.selected?.id === entry.id}>
        <button
          type="button"
          class="name"
          aria-current={files.selected?.id === entry.id}
          aria-label={t('files.select', { name: entry.name })}
          onclick={() => files.select(entry.id)}
        >
          <Icon name={entry.kind === 'pdf' ? 'file-pdf' : 'image'} />
          <span>{entry.name}</span>
          {#if entry.problem?.code === 'QW-PDF-ENCRYPTED' || entry.report?.locked}
            <Icon name="lock" />
          {/if}
        </button>
        <span class="status" class:bad={['wrong-password', 'damaged', 'out-of-range', 'failed'].includes(entry.status)}>
          {tDynamic(`status.${entry.status}`)}
        </span>
        <span class="move">
          <button
            type="button"
            aria-label={t('files.moveUp', { name: entry.name })}
            disabled={index === 0 || jobs.running}
            onclick={() => files.move(entry.id, -1)}><Icon name="arrow-up" /></button>
          <button
            type="button"
            aria-label={t('files.moveDown', { name: entry.name })}
            disabled={index === files.entries.length - 1 || jobs.running}
            onclick={() => files.move(entry.id, 1)}><Icon name="arrow-down" /></button>
          <button
            type="button"
            aria-label={t('files.remove', { name: entry.name })}
            disabled={jobs.running}
            onclick={() => files.remove(entry.id)}><Icon name="x" /></button>
        </span>
        {#if entry.progress}
          <div class="progress">
            <ProgressBar
              current={entry.progress.done}
              total={entry.progress.total}
              label={t('queue.runningLabel', { name: entry.name })}
            />
          </div>
        {/if}
        {#if entry.error}
          <p class="error">{entry.error.message} <span class="mono">({entry.error.code})</span></p>
        {/if}
      </li>
    {/each}
  </ul>
</section>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
    margin-bottom: var(--space-4);
  }
  h2 {
    margin: 0;
  }
  .count {
    color: var(--text-muted);
    font-weight: 400;
  }
  .actions {
    display: flex;
    gap: var(--space-3);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    background: var(--surface);
  }
  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  li:last-child {
    border-bottom: 0;
  }
  li.selected {
    background: var(--selection);
  }
  .name {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 32px;
    padding: 0;
    border: 0;
    background: transparent;
    text-align: left;
    overflow: hidden;
  }
  .name span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .status {
    color: var(--text-muted);
  }
  .status.bad {
    color: var(--danger);
  }
  .move {
    display: flex;
  }
  .move button {
    min-width: var(--target-min);
    min-height: var(--target-min);
    border: 0;
    background: transparent;
    display: grid;
    place-items: center;
  }
  .move button:disabled {
    color: var(--text-muted);
    cursor: not-allowed;
  }
  .progress,
  .error {
    grid-column: 1 / -1;
    margin: 0;
  }
  .error {
    color: var(--danger);
  }
</style>
