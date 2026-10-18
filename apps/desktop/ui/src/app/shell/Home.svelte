<script lang="ts">
  import { t } from '../../lib/i18n';
  import DropZone from '../../lib/components/DropZone/DropZone.svelte';
  import Recent from '../../lib/features/recent/View.svelte';

  interface Props {
    dragging: boolean;
    onchoose: () => void;
  }

  let { dragging, onchoose }: Props = $props();
</script>

<div class="home">
  <DropZone active={dragging} {onchoose} />
  <p class="note">{t('about.summary')}</p>
  <Recent />
</div>

<style>
  .home {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  .note {
    margin: 0;
    color: var(--text-muted);
    max-width: 70ch;
  }
</style>
