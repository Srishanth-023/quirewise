<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../lib/i18n';
  import { ipc, onLaunch, toAppError, type Launch } from '../lib/ipc/client';
  import { listenForDrops } from '../lib/dnd';
  import { interpretKey, isInteractive } from '../lib/shortcuts';
  import CommandPalette from '../lib/components/CommandPalette/CommandPalette.svelte';
  import { buildCommands } from '../lib/components/CommandPalette/buildCommands';
  import type { RecentFile } from '../lib/ipc/client';
  import Toast from '../lib/components/Toast/Toast.svelte';
  import Settings from '../lib/features/settings/View.svelte';
  import Recipes from '../lib/features/recipes/View.svelte';
  import { recipes } from '../lib/features/recipes/recipes.svelte';
  import Shortcuts from '../lib/features/shortcuts/View.svelte';
  import { files } from '../lib/stores/files.svelte';
  import { jobs } from '../lib/stores/jobs.svelte';
  import { settings } from '../lib/stores/settings.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { tools } from '../lib/stores/tools.svelte';
  import { toolOptions } from '../lib/stores/options.svelte';
  import { runsWithoutFiles } from '../lib/forms/toolForms';
  import About from './shell/About.svelte';
  import CancelDialog from './shell/CancelDialog.svelte';
  import Home from './shell/Home.svelte';
  import Legal from './shell/Legal.svelte';
  import Nav from './shell/Nav.svelte';
  import RightPanel from './shell/RightPanel.svelte';
  import Workspace from './shell/Workspace.svelte';
  import licenseText from '../../../../../LICENSE?raw';
  import privacyText from '../../../../../docs/legal/PRIVACY.md?raw';
  import termsText from '../../../../../docs/legal/TERMS.md?raw';

  type Route = 'home' | 'recipes' | 'settings' | 'about' | 'license' | 'privacy' | 'terms';

  let route = $state<Route>('home');
  let paletteOpen = $state(false);
  let shortcutsOpen = $state(false);
  let confirmCancel = $state(false);
  let dragging = $state(false);
  let recent = $state<RecentFile[]>([]);

  const dialogOpen = $derived(paletteOpen || shortcutsOpen || confirmCancel);

  async function pickAndAdd(): Promise<void> {
    try {
      const picked = await ipc.pickFiles();
      route = 'home';
      await files.add(picked);
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  async function openPalette(): Promise<void> {
    recent = await ipc.recentList().catch(() => []);
    paletteOpen = true;
  }

  /** Files and a tool named by an Explorer entry, at start or from a second start. */
  function applyLaunch(launch: Launch): void {
    if (launch.files.length === 0 && !launch.tool) return;
    route = 'home';
    if (launch.tool) tools.select(launch.tool);
    if (launch.files.length > 0) void files.add(launch.files);
  }

  function show(next: Route): void {
    route = next;
  }

  const commands = $derived(
    buildCommands(tools.items, recent, {
      selectTool: (id) => {
        show('home');
        tools.select(id);
      },
      addFile: (path) => {
        show('home');
        void files.add([path]);
      },
      openFiles: () => void pickAndAdd(),
      showShortcuts: () => (shortcutsOpen = true),
      openSettings: () => show('settings'),
      useRecipe: (id) => {
        show('recipes');
        recipes.chosen = id;
      },
      openAbout: () => show('about'),
      clearFiles: () => files.clear(),
    },
    recipes.list),
  );

  function onkeydown(event: KeyboardEvent): void {
    const action = interpretKey(event, {
      dialogOpen,
      jobRunning: jobs.running,
      canRun: tools.selected !== undefined && files.entries.length > 0,
      focusIsInteractive: isInteractive(document.activeElement),
    });
    if (!action) return;
    event.preventDefault();
    if (action === 'palette') void openPalette();
    else if (action === 'open-files') void pickAndAdd();
    else if (action === 'shortcuts') shortcutsOpen = true;
    else if (action === 'confirm-cancel') confirmCancel = true;
    else if (action === 'run' && tools.selected) void jobs.run(tools.selected);
  }

  async function cancelJob(): Promise<void> {
    confirmCancel = false;
    await jobs.cancel();
  }

  onMount(() => {
    void tools.load();
    void recipes.load();
    void settings.load();
    void jobs.listen();
    void ipc.takeLaunch().then(applyLaunch).catch(() => undefined);
    let stopLaunch: (() => void) | undefined;
    void onLaunch(applyLaunch)
      .then((unlisten) => (stopLaunch = unlisten))
      .catch(() => undefined);
    let stop: (() => void) | undefined;
    void listenForDrops({
      onhover: (hovering) => (dragging = hovering),
      ondrop: (paths) => {
        route = 'home';
        void files.add(paths);
      },
    })
      .then((unlisten) => (stop = unlisten))
      // Outside the desktop shell (plain browser during UI work) there is no webview to listen on.
      .catch(() => undefined);
    return () => {
      stop?.();
      stopLaunch?.();
      jobs.stop();
    };
  });
  /** A tool such as HTML to PDF can run on a web address alone, so the run bar shows without files. */
  const withoutFiles = (): boolean => {
    void toolOptions.edits;
    return tools.selected ? runsWithoutFiles(tools.selected.id, toolOptions.values(tools.selected)) : false;
  };
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <Nav {route} onroute={(next) => (route = next)} />

  <main class="workspace">
    {#if route === 'home'}
      {#if files.entries.length === 0 && !withoutFiles()}
        <Home {dragging} onchoose={() => void pickAndAdd()} />
      {:else}
        <Workspace
          onadd={() => void pickAndAdd()}
          oncancel={() => (confirmCancel = true)}
          onanother={() => void openPalette()}
        />
      {/if}
    {:else if route === 'recipes'}
      <Recipes />
    {:else if route === 'settings'}
      <Settings />
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

  <RightPanel />
</div>

{#if dragging}
  <div class="overlay" aria-hidden="true"><p>{t('drop.overlay')}</p></div>
{/if}

<CommandPalette open={paletteOpen} {commands} onclose={() => (paletteOpen = false)} />
<Shortcuts open={shortcutsOpen} onclose={() => (shortcutsOpen = false)} />
<CancelDialog open={confirmCancel} onclose={() => (confirmCancel = false)} onconfirm={() => void cancelJob()} />
<Toast />

<style>
  .shell {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr) 320px;
    height: 100%;
  }
  .shell > :global(.nav) {
    border-inline-end: 1px solid var(--border);
  }
  .shell > :global(.panel) {
    border-inline-start: 1px solid var(--border);
  }
  .workspace {
    padding: var(--space-6);
    overflow: auto;
  }
  .overlay {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--selection);
    opacity: 0.92;
    pointer-events: none;
    z-index: 5;
  }
  .overlay p {
    margin: 0;
    padding: var(--space-6) var(--space-8);
    border: 2px dashed var(--accent);
    border-radius: var(--radius-panel);
    background: var(--surface);
    font-size: var(--text-lg);
  }
  @media (max-width: 1099px) {
    /* Stacked: every part takes the height of its content and the whole shell scrolls, so the
       tool list is never squeezed under the next block. */
    .shell {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto auto auto;
      align-content: start;
      overflow: auto;
    }
    .shell > :global(.nav),
    .shell > :global(.panel),
    .workspace {
      overflow: visible;
    }
    .shell :global(.nav .tools) {
      flex: none;
    }
    .shell > :global(.nav) {
      border-inline-end: 0;
      border-bottom: 1px solid var(--border);
    }
    .shell > :global(.panel) {
      border-inline-start: 0;
      border-top: 1px solid var(--border);
    }
  }
</style>
