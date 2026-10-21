<script lang="ts">
  import { t } from '../../i18n';
  import Button from '../../components/Button/Button.svelte';
  import Icon from '../../components/Icon.svelte';
  import { files } from '../../stores/files.svelte';
  import { jobs } from '../../stores/jobs.svelte';
  import { tools, toolName } from '../../stores/tools.svelte';
  import { runsWithoutFiles } from '../../forms/toolForms';
  import { toolOptions } from '../../stores/options.svelte';

  interface Props {
    oncancel: () => void;
  }

  let { oncancel }: Props = $props();

  const tool = $derived(tools.selected);
  /** A tool may need no files, such as printing a web address. */
  const noFiles = (): boolean => {
    void toolOptions.edits;
    return tool ? runsWithoutFiles(tool.id, toolOptions.values(tool)) : false;
  };
  const canRun = (): boolean => !!tool && (files.entries.length > 0 || noFiles());
  /** Read out by screen readers when a job ends, since the list changes without moving focus. */
  const announcement = $derived.by(() => {
    if (jobs.running || !jobs.status || jobs.status === 'running') return '';
    const done = files.entries.filter((e) => e.status === 'done').length;
    return t('queue.finished', { done, failed: files.entries.filter((e) => e.status !== 'done').length });
  });
  const hint = $derived(files.entries.length === 0 ? t('queue.noFiles') : tool ? '' : t('queue.noTool'));
</script>

<div class="bar">
  {#if jobs.running}
    <Button onclick={oncancel}><Icon name="stop" /> {t('queue.cancel')}</Button>
  {:else}
    <Button
      variant="primary"
      disabled={!canRun()}
      onclick={() => tool && void jobs.run(tool)}
    >
      <Icon name="play" />
      {#if tool}
        {#if files.entries.length === 0}
          {t('queue.runAddress', { tool: toolName(tool) })}
        {:else}
          {t('queue.run', { tool: toolName(tool), count: files.entries.length })}
        {/if}
      {:else}
        {t('queue.noTool')}
      {/if}
    </Button>
    {#if jobs.hasFailures}
      <Button onclick={() => void jobs.retryFailed()}><Icon name="arrow-clockwise" /> {t('queue.retry')}</Button>
    {/if}
  {/if}
  {#if hint && !tool}<span class="hint">{hint}</span>{/if}
  <p class="sr" role="status">{announcement}</p>
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .hint {
    color: var(--text-muted);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: 0;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
