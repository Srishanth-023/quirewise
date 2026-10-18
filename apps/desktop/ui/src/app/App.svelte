<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../lib/i18n';
  import { ipc, toAppError } from '../lib/ipc/client';
  import { listenForDrops } from '../lib/dnd';
  import { interpretKey, isInteractive } from '../lib/shortcuts';
  import CommandPalette from '../lib/components/CommandPalette/CommandPalette.svelte';
  import { buildCommands } from '../lib/components/CommandPalette/buildCommands';
  import type { RecentFile } from '../lib/ipc/client';
  import Toast from '../lib/components/Toast/Toast.svelte';
  import Settings from '../lib/features/settings/View.svelte';
  import Shortcuts from '../lib/features/shortcuts/View.svelte';
  import { files } from '../lib/stores/files.svelte';
  import { jobs } from '../lib/stores/jobs.svelte';
  import { settings } from '../lib/stores/settings.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { tools } from '../lib/stores/tools.svelte';
  import About from './shell/About.svelte';
  import CancelDialog from './shell/CancelDialog.svelte';
  import Home from './shell/Home.svelte';
  import Legal from './shell/Legal.svelte';
  import Nav from './shell/Nav.svelte';
  import RightPanel from './shell/RightPanel.svelte';
  import Workspace from './shell/Workspace.svelte';
  import licenseText from '../../../../../LICENSE?raw';
  import privacyText from '../../../../../PRIVACY.md?raw';
  import termsText from '../../../../../TERMS.md?raw';

  type Route = 'home' | 'settings' | 'about' | 'license' | 'privacy' | 'terms';

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
      openAbout: () => show('about'),
      clearFiles: () => files.clear(),
    }),
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
    void settings.load();
    void jobs.listen();
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
      jobs.stop();
    };
  });
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <Nav {route} onroute={(next) => (route = next)} />

  <main class="workspace">
    {#if route === 'home'}
      {#if files.entries.length === 0}
        <Home {dragging} onchoose={() => void pickAndAdd()} />
      {:else}
        <Workspace
          onadd={() => void pickAndAdd()}
          oncancel={() => (confirmCancel = true)}
          onanother={() => void openPalette()}
        />
      {/if}
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
    border-right: 1px solid var(--border);
  }
  .shell > :global(.panel) {
    border-left: 1px solid var(--border);
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
    .shell {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr) auto;
      overflow: auto;
    }
    .shell > :global(.nav) {
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }
    .shell > :global(.panel) {
      border-left: 0;
      border-top: 1px solid var(--border);
    }
  }
</style>
