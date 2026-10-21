<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../../i18n';
  import Button from '../../components/Button/Button.svelte';
  import { ipc, toAppError } from '../../ipc/client';
  import { settings } from '../../stores/settings.svelte';
  import { toasts } from '../../stores/toasts.svelte';
  import { toolName, tools } from '../../stores/tools.svelte';

  /** The most tools the submenu offers. The core checks the same limit. */
  const MAX = 5;

  let installed = $state(false);
  let offered = $state<string[]>([]);
  let chosen = $state<string[]>([]);
  let busy = $state(false);

  const candidates = $derived(tools.items.filter((tool) => offered.includes(tool.id)));
  const atLimit = $derived(chosen.length >= MAX);

  async function refresh(): Promise<void> {
    try {
      [installed, offered] = await Promise.all([ipc.explorerStatus(), ipc.explorerTools()]);
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  function toggle(id: string, on: boolean): void {
    chosen = on ? [...chosen, id] : chosen.filter((x) => x !== id);
  }

  async function apply(): Promise<void> {
    busy = true;
    try {
      const items = chosen.flatMap((id) => {
        const tool = tools.items.find((x) => x.id === id);
        return tool ? [{ tool: tool.id, title: toolName(tool) }] : [];
      });
      await ipc.explorerEnable(items as Parameters<typeof ipc.explorerEnable>[0], t('explorer.open'));
      await settings.load();
      installed = true;
      toasts.info(t('explorer.added'));
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      busy = false;
    }
  }

  async function remove(): Promise<void> {
    busy = true;
    try {
      await ipc.explorerDisable();
      installed = false;
      toasts.info(t('explorer.removed'));
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    // Start from the tools saved the last time the menu was added.
    if (chosen.length === 0 && settings.value?.explorer_tools) chosen = [...settings.value.explorer_tools];
  });

  onMount(() => void refresh());
</script>

<section aria-labelledby="explorer-title">
  <h2 id="explorer-title">{t('explorer.title')}</h2>
  <p class="help">{t('explorer.help')}</p>

  <fieldset>
    <legend>{t('explorer.tools', { max: MAX })}</legend>
    {#each candidates as tool (tool.id)}
      <label class="check">
        <input
          type="checkbox"
          checked={chosen.includes(tool.id)}
          disabled={busy || (atLimit && !chosen.includes(tool.id))}
          onchange={(e) => toggle(tool.id, e.currentTarget.checked)}
        />
        {toolName(tool)}
      </label>
    {/each}
  </fieldset>

  <div class="actions">
    <Button variant="primary" disabled={busy || chosen.length === 0} onclick={() => void apply()}>
      {installed ? t('explorer.update') : t('explorer.add')}
    </Button>
    {#if installed}
      <Button disabled={busy} onclick={() => void remove()}>{t('explorer.remove')}</Button>
    {/if}
  </div>
  <p class="help" role="status">{installed ? t('explorer.on') : t('explorer.off')}</p>
</section>

<style>
  section {
    margin-top: var(--space-6);
    max-width: 60ch;
  }
  h2 {
    margin: 0 0 var(--space-3);
  }
  fieldset {
    margin: var(--space-4) 0;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    max-height: 260px;
    overflow: auto;
  }
  legend {
    padding: 0 var(--space-2);
    font-weight: 500;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--target-min);
  }
  .actions {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .help {
    margin: var(--space-2) 0;
    color: var(--text-muted);
  }
</style>
