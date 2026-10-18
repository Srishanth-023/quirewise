<script lang="ts">
  import Thumbnail from '../Thumbnail/Thumbnail.svelte';

  interface Props {
    path: string;
    password?: string | null;
    pageCount: number;
    edge?: number;
  }

  let { path, password = null, pageCount, edge = 160 }: Props = $props();

  const pages = $derived(Array.from({ length: pageCount }, (_, i) => i));
</script>

<ul class="grid">
  {#each pages as page (page)}
    <li><Thumbnail {path} {password} {page} {edge} /></li>
  {/each}
</ul>

<style>
  .grid {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: var(--space-5);
    align-items: start;
  }
</style>
