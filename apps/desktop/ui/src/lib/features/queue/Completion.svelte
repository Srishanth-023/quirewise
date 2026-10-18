<script lang="ts">
  import { t } from '../../i18n';
  import { ipc, toAppError } from '../../ipc/client';
  import Button from '../../components/Button/Button.svelte';
  import { files } from '../../stores/files.svelte';
  import { jobs } from '../../stores/jobs.svelte';
  import { toasts } from '../../stores/toasts.svelte';

  interface Props {
    /** Opens the command palette so the user can pick another tool for the results. */
    onanother: () => void;
  }

  let { onanother }: Props = $props();

  function baseName(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
  }

  async function run(action: (path: string) => Promise<void>, path: string): Promise<void> {
    try {
      await action(path);
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  /** Replaces the work list with the result files, ready for the next tool. */
  async function useResults(): Promise<void> {
    const paths = jobs.results.map((r) => r.path);
    files.clear();
    await files.add(paths);
    onanother();
  }
</script>

{#if !jobs.running && jobs.results.length > 0}
  <section aria-labelledby="result-title">
    <h2 id="result-title">{t('result.title')}</h2>
    <ul>
      {#each jobs.results as result (result.path)}
        <li>
          <span class="name">{t('result.saved', { name: baseName(result.path) })}</span>
          <Button onclick={() => void run(ipc.openOutput, result.path)}>{t('result.open')}</Button>
          <Button onclick={() => void run(ipc.revealOutput, result.path)}>{t('result.reveal')}</Button>
        </li>
      {/each}
    </ul>
    <Button variant="primary" onclick={() => void useResults()}>{t('result.another')}</Button>
  </section>
{/if}

<style>
  section {
    padding: var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-panel);
    background: var(--surface);
  }
  h2 {
    margin: 0 0 var(--space-3);
  }
  ul {
    list-style: none;
    margin: 0 0 var(--space-4);
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
    flex-wrap: wrap;
  }
  .name {
    flex: 1;
    overflow-wrap: anywhere;
  }
</style>
