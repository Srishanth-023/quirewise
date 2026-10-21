<script lang="ts">
  import { t, tDynamic } from '../../lib/i18n';
  import Icon from '../../lib/components/Icon.svelte';
  import { tools, toolName } from '../../lib/stores/tools.svelte';

  type Route = 'home' | 'recipes' | 'settings' | 'about' | 'license' | 'privacy' | 'terms';

  interface Props {
    route: Route;
    onroute: (route: Route) => void;
  }

  let { route, onroute }: Props = $props();

  let query = $state('');
  const matches = $derived(tools.grouped(tools.search(query)));

  const links: { route: Route; label: 'nav.home' | 'nav.recipes' | 'nav.settings' | 'nav.about' | 'nav.license' | 'nav.privacy' | 'nav.terms' }[] = [
    { route: 'home', label: 'nav.home' },
    { route: 'recipes', label: 'nav.recipes' },
    { route: 'settings', label: 'nav.settings' },
    { route: 'about', label: 'nav.about' },
    { route: 'license', label: 'nav.license' },
    { route: 'privacy', label: 'nav.privacy' },
    { route: 'terms', label: 'nav.terms' },
  ];
</script>

<nav class="nav" aria-label={t('nav.tools')}>
  <h2 class="brand"><Icon name="file-pdf" size={20} /> {t('app.name')}</h2>

  <div class="search">
    <Icon name="magnifying-glass" />
    <input type="search" bind:value={query} placeholder={t('tools.search')} aria-label={t('tools.search')} />
  </div>

  <div class="tools">
    {#if tools.loaded && tools.items.length === 0}
      <p class="muted">{t('tools.noneRegistered')}</p>
    {:else if matches.length === 0 && tools.items.length > 0}
      <p class="muted">{t('tools.noMatch')}</p>
    {/if}
    {#each matches as { group, tools: groupTools } (group)}
      <h3>{tDynamic(`tools.group.${group}`, group)}</h3>
      <ul>
        {#each groupTools as tool (tool.id)}
          <li>
            <button
              type="button"
              class:current={tools.current === tool.id}
              aria-pressed={tools.current === tool.id}
              onclick={() => tools.select(tools.current === tool.id ? null : tool.id)}
            >
              {toolName(tool)}
            </button>
          </li>
        {/each}
      </ul>
    {/each}
  </div>

  <ul class="links">
    {#each links as link (link.route)}
      <li>
        <button type="button" class:current={route === link.route} onclick={() => onroute(link.route)}>
          {t(link.label)}
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  .nav {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5);
    background: var(--surface);
    overflow: auto;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: 0;
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
  }
  input {
    flex: 1;
    min-height: 32px;
    border: 0;
    background: transparent;
    min-width: 0;
  }
  input:focus-visible {
    outline-offset: 4px;
  }
  .tools {
    flex: 1;
    min-height: 0;
  }
  h3 {
    margin: var(--space-4) 0 var(--space-2);
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: 600;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li button {
    width: 100%;
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: transparent;
    text-align: start;
  }
  li button:hover {
    background: var(--surface-2);
  }
  li button.current {
    background: var(--selection);
    border-color: var(--border);
    font-weight: 600;
  }
  .muted {
    color: var(--text-muted);
  }
  .links {
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
  }
</style>
