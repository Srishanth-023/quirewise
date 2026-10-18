<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    open: boolean;
    title: string;
    onclose: () => void;
    children: Snippet;
    /** Wider dialogs for lists such as the command palette. */
    wide?: boolean;
  }

  let { open, title, onclose, children, wide = false }: Props = $props();

  let dialog: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<!-- The native dialog traps focus, restores it on close and closes on Escape. -->
<dialog bind:this={dialog} class:wide aria-label={title} onclose={onclose} oncancel={onclose}>
  {#if open}
    <h2>{title}</h2>
    {@render children()}
  {/if}
</dialog>

<style>
  dialog {
    width: min(480px, calc(100vw - var(--space-7)));
    padding: var(--space-5);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-panel);
    background: var(--surface);
    color: var(--text);
  }
  dialog.wide {
    width: min(640px, calc(100vw - var(--space-7)));
  }
  dialog::backdrop {
    background: var(--bg);
    opacity: 0.8;
  }
  h2 {
    margin: 0 0 var(--space-4);
  }
</style>
