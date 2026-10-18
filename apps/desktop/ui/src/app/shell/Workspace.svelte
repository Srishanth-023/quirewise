<script lang="ts">
  import Queue from '../../lib/features/queue/View.svelte';
  import RunBar from '../../lib/features/queue/RunBar.svelte';
  import Completion from '../../lib/features/queue/Completion.svelte';
  import Viewer from '../../lib/features/viewer/View.svelte';

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
  <Viewer />
</div>

<style>
  .workspace {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }
</style>
