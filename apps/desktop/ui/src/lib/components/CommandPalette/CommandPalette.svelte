<script lang="ts">
  import { tick } from 'svelte';
  import { t } from '../../i18n';
  import Dialog from '../Dialog/Dialog.svelte';
  import { rankCommands, type Command } from './commands';

  interface Props {
    open: boolean;
    commands: Command[];
    onclose: () => void;
  }

  let { open, commands, onclose }: Props = $props();

  let query = $state('');
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();

  const results = $derived(rankCommands(commands, query));
  const groups = ['tools', 'recipes', 'files', 'actions'] as const;
  const activeId = $derived(results[active]?.id);

  $effect(() => {
    if (open) {
      query = '';
      active = 0;
      void tick().then(() => input?.focus());
    }
  });

  $effect(() => {
    // Keep the highlighted row valid when the list shrinks.
    if (active >= results.length) active = Math.max(0, results.length - 1);
  });

  function choose(command: Command | undefined): void {
    if (!command) return;
    onclose();
    command.run();
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      active = results.length ? (active + 1) % results.length : 0;
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      active = results.length ? (active - 1 + results.length) % results.length : 0;
    } else if (event.key === 'Enter') {
      event.preventDefault();
      choose(results[active]);
    }
  }
</script>

<Dialog {open} title={t('palette.title')} {onclose} wide>
  <input
    bind:this={input}
    bind:value={query}
    {onkeydown}
    type="text"
    role="combobox"
    aria-expanded="true"
    aria-controls="palette-list"
    aria-activedescendant={activeId ? `palette-${activeId}` : undefined}
    aria-label={t('palette.placeholder')}
    placeholder={t('palette.placeholder')}
    autocomplete="off"
  />
  <div id="palette-list" role="listbox" aria-label={t('palette.title')}>
    {#each groups as group (group)}
      {@const items = results.filter((c) => c.group === group)}
      {#if items.length > 0}
        <p class="group">{t(`palette.group.${group}`)}</p>
        {#each items as command (command.id)}
          <div
            id={`palette-${command.id}`}
            class="row"
            class:active={activeId === command.id}
            role="option"
            aria-selected={activeId === command.id}
            tabindex="-1"
            onclick={() => choose(command)}
            onkeydown={(e) => e.key === 'Enter' && choose(command)}
          >
            <span>{command.label}</span>
            {#if command.hint}<span class="hint mono">{command.hint}</span>{/if}
          </div>
        {/each}
      {/if}
    {/each}
    {#if results.length === 0}
      <p class="empty">{t('palette.empty')}</p>
    {/if}
  </div>
</Dialog>

<style>
  input {
    width: 100%;
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
  }
  #palette-list {
    margin-top: var(--space-3);
    max-height: 50vh;
    overflow: auto;
  }
  .group {
    margin: var(--space-3) 0 var(--space-2);
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: 600;
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: var(--space-4);
    min-height: 32px;
    align-items: center;
    padding: 0 var(--space-3);
    border-radius: var(--radius-control);
    cursor: pointer;
  }
  .row.active {
    background: var(--selection);
    outline: 1px solid var(--control-border);
  }
  .hint {
    color: var(--text-muted);
  }
  .empty {
    color: var(--text-muted);
  }
</style>
