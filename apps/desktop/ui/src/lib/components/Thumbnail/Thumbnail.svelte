<script lang="ts">
  import { onMount } from 'svelte';
  import { toAppError } from '../../ipc/client';
  import { t } from '../../i18n';
  import { thumbnail, type Thumb } from './thumbnails';

  interface Props {
    path: string;
    password?: string | null;
    /** Zero-based page index. */
    page: number;
    /** Longest edge in pixels. */
    edge?: number;
    onloaded?: (thumb: Thumb) => void;
  }

  let { path, password = null, page, edge = 160, onloaded }: Props = $props();

  let host: HTMLElement | undefined = $state();
  let thumb = $state<Thumb | null>(null);
  let failed = $state(false);
  let started = false;

  async function load(): Promise<void> {
    if (started) return;
    started = true;
    try {
      thumb = await thumbnail(path, password, page, edge);
      onloaded?.(thumb);
    } catch (error) {
      failed = true;
      void toAppError(error);
    }
  }

  onMount(() => {
    if (!host || typeof IntersectionObserver === 'undefined') {
      void load();
      return;
    }
    // Render only pages that scroll into view, so a 500-page file does not queue 500 renders.
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          observer.disconnect();
          void load();
        }
      },
      { rootMargin: '200px' },
    );
    observer.observe(host);
    return () => observer.disconnect();
  });
</script>

<figure class="thumb" bind:this={host}>
  {#if thumb}
    <img src={thumb.url} width={thumb.width} height={thumb.height} alt={t('viewer.page', { n: page + 1 })} />
  {:else}
    <div class="blank" class:failed role="img" aria-label={t('viewer.page', { n: page + 1 })}>
      {#if failed}{t('viewer.failed')}{/if}
    </div>
  {/if}
  <figcaption class="mono">{page + 1}</figcaption>
</figure>

<style>
  .thumb {
    margin: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
  }
  img,
  .blank {
    display: block;
    max-width: 100%;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-thumbnail);
    background: var(--surface-2);
  }
  .blank {
    width: 120px;
    aspect-ratio: 612 / 792;
    display: grid;
    place-items: center;
    color: var(--text-muted);
    font-size: var(--text-xs);
    text-align: center;
  }
  figcaption {
    color: var(--text-muted);
  }
</style>
