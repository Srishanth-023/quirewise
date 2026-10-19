<script lang="ts">
  import Queue from '../../lib/features/queue/View.svelte';
  import RunBar from '../../lib/features/queue/RunBar.svelte';
  import Completion from '../../lib/features/queue/Completion.svelte';
  import Viewer from '../../lib/features/viewer/View.svelte';
  import Organizer from '../../lib/features/organize/View.svelte';
  import Editor from '../../lib/features/editor/EditorView.svelte';
  import { editorTools } from '../../lib/features/editor/model';
  import { tools } from '../../lib/stores/tools.svelte';

  interface Props {
    onadd: () => void;
    oncancel: () => void;
    onanother: () => void;
  }

  let { onadd, oncancel, onanother }: Props = $props();
</script>

<div class="workspace">
  <Queue {onadd} />
  <RunBar {oncancel} />
  <Completion {onanother} />
  {#if tools.selected?.id === 'organize'}
    <Organizer tool={tools.selected} />
  {:else if tools.selected && editorTools.includes(tools.selected.id)}
    <Editor tool={tools.selected} />
  {:else}
    <Viewer />
  {/if}
</div>

<style>
  .workspace {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }
</style>
