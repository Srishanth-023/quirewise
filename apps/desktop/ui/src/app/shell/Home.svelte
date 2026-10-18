<script lang="ts">
  import { t } from '../../lib/i18n';
  import DropZone from '../../lib/components/DropZone/DropZone.svelte';

  // File picking arrives with the typed IPC in Phase 2; the zone is real, the action is pending.
  function choose(): void {}
</script>

<DropZone onchoose={choose} />
<p class="note">{t('about.summary')}</p>

<style>
  .note {
    color: var(--text-muted);
    max-width: 70ch;
  }
</style>
