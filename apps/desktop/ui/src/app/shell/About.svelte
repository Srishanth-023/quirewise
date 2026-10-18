<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n';
  import { ipc } from '../../lib/ipc/client';

  let version = $state('');

  onMount(async () => {
    try {
      version = await ipc.appVersion();
    } catch {
      version = '';
    }
  });
</script>

<article>
  <h1>{t('app.name')}</h1>
  {#if version}
    <p class="mono">{t('about.version', { version })}</p>
  {/if}
  <p>{t('about.summary')}</p>
</article>
