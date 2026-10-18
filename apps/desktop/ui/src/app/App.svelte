<script lang="ts">
  import { t } from '../lib/i18n';
  import Icon from '../lib/components/Icon.svelte';
  import About from './shell/About.svelte';
  import Home from './shell/Home.svelte';
  import Legal from './shell/Legal.svelte';
  import licenseText from '../../../../../LICENSE?raw';
  import privacyText from '../../../../../PRIVACY.md?raw';
  import termsText from '../../../../../TERMS.md?raw';

  type Route = 'home' | 'about' | 'license' | 'privacy' | 'terms';

  let route = $state<Route>('home');

  const links: { route: Route; label: 'nav.home' | 'nav.about' | 'nav.license' | 'nav.privacy' | 'nav.terms' }[] = [
    { route: 'home', label: 'nav.home' },
    { route: 'about', label: 'nav.about' },
    { route: 'license', label: 'nav.license' },
    { route: 'privacy', label: 'nav.privacy' },
    { route: 'terms', label: 'nav.terms' },
  ];
</script>

<div class="shell">
  <nav class="nav" aria-label={t('nav.tools')}>
    <h2 class="brand"><Icon name="file-pdf" size={20} /> {t('app.name')}</h2>
    <p class="empty">{t('tools.empty')}</p>
    <ul class="links">
      {#each links as link (link.route)}
        <li>
          <button class:current={route === link.route} onclick={() => (route = link.route)}>
            {t(link.label)}
          </button>
        </li>
      {/each}
    </ul>
  </nav>

  <main class="workspace">
    {#if route === 'home'}
      <Home />
    {:else if route === 'about'}
      <About />
    {:else if route === 'license'}
      <Legal title={t('nav.license')} body={licenseText} />
    {:else if route === 'privacy'}
      <Legal title={t('nav.privacy')} body={privacyText} />
    {:else}
      <Legal title={t('nav.terms')} body={termsText} />
    {/if}
  </main>

  <aside class="options" aria-label={t('options.title')}>
    <h2>{t('options.title')}</h2>
    <p class="empty">{t('options.empty')}</p>
  </aside>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr) 320px;
    height: 100%;
  }
  .nav,
  .options {
    padding: var(--space-5);
    background: var(--surface);
    overflow: auto;
  }
  .nav {
    border-right: 1px solid var(--border);
  }
  .options {
    border-left: 1px solid var(--border);
  }
  .workspace {
    padding: var(--space-6);
    overflow: auto;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: 0 0 var(--space-5);
  }
  .empty {
    color: var(--text-muted);
  }
  .links {
    list-style: none;
    margin: var(--space-6) 0 0;
    padding: 0;
  }
  .links button {
    width: 100%;
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: transparent;
    text-align: left;
  }
  .links button:hover {
    background: var(--surface-2);
  }
  .links button.current {
    background: var(--selection);
    border-color: var(--border);
    font-weight: 600;
  }
  @media (max-width: 1099px) {
    .shell {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr) auto;
    }
    .nav {
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }
    .options {
      border-left: 0;
      border-top: 1px solid var(--border);
    }
  }
</style>
