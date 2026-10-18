<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../../i18n';
  import { ipc, toAppError, type RecentFile } from '../../ipc/client';
  import Button from '../../components/Button/Button.svelte';
  import Thumbnail from '../../components/Thumbnail/Thumbnail.svelte';
  import { files } from '../../stores/files.svelte';
  import { toasts } from '../../stores/toasts.svelte';

  let recent = $state<RecentFile[]>([]);

  export async function refresh(): Promise<void> {
    try {
      recent = await ipc.recentList();
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  async function clear(): Promise<void> {
    try {
      await ipc.recentClear();
      recent = [];
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  onMount(() => void refresh());
</script>

<section aria-labelledby="recent-title">
  <div class="head">
    <h2 id="recent-title">{t('recent.title')}</h2>
    {#if recent.length > 0}
      <Button onclick={() => void clear()}>{t('recent.clear')}</Button>
    {/if}
  </div>

  {#if recent.length === 0}
    <p class="empty">{t('recent.empty')}</p>
  {:else}
    <ul class="grid">
      {#each recent as file (file.path)}
        <li>
          <button type="button" aria-label={t('recent.open', { name: file.name })} onclick={() => void files.add([file.path])}>
            <Thumbnail path={file.path} page={0} edge={200} />
            <span class="name">{file.name}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-4);
  }
  h2 {
    margin: 0;
  }
  .empty {
    color: var(--text-muted);
  }
  .grid {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-5);
  }
  button {
    width: 100%;
    padding: var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--radius-panel);
    background: transparent;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
  }
  button:hover {
    background: var(--surface);
    border-color: var(--border);
  }
  .name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
