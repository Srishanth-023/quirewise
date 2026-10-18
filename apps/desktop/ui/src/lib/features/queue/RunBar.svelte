<script lang="ts">
  import { t } from '../../i18n';
  import Button from '../../components/Button/Button.svelte';
  import Icon from '../../components/Icon.svelte';
  import { files } from '../../stores/files.svelte';
  import { jobs } from '../../stores/jobs.svelte';
  import { tools, toolName } from '../../stores/tools.svelte';

  interface Props {
    oncancel: () => void;
  }

  let { oncancel }: Props = $props();

  const tool = $derived(tools.selected);
  const hint = $derived(
    files.entries.length === 0 ? t('queue.noFiles') : tool ? '' : t('queue.noTool'),
  );
</script>

<div class="bar">
  {#if jobs.running}
    <Button onclick={oncancel}><Icon name="stop" /> {t('queue.cancel')}</Button>
  {:else}
    <Button
      variant="primary"
      disabled={!tool || files.entries.length === 0}
      onclick={() => tool && void jobs.run(tool)}
    >
      <Icon name="play" />
      {#if tool}
        {t('queue.run', { tool: toolName(tool), count: files.entries.length })}
      {:else}
        {t('queue.noTool')}
      {/if}
    </Button>
    {#if jobs.hasFailures}
      <Button onclick={() => void jobs.retryFailed()}><Icon name="arrow-clockwise" /> {t('queue.retry')}</Button>
    {/if}
  {/if}
  {#if hint && !tool}<span class="hint">{hint}</span>{/if}
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
</style>
