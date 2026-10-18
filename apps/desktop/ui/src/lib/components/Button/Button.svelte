<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    variant?: 'primary' | 'secondary';
    disabled?: boolean;
    type?: 'button' | 'submit';
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
  }

  let { variant = 'secondary', disabled = false, type = 'button', onclick, children }: Props =
    $props();
</script>

<button class={variant} {type} {disabled} {onclick}>
  {@render children()}
</button>

<style>
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    min-height: 32px;
    min-width: var(--target-min);
    padding: 0 var(--space-5);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--text);
    font-weight: 500;
    transition:
      background-color var(--motion-fast) var(--ease),
      border-color var(--motion-fast) var(--ease);
  }
  button:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
  }
  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }
  button:disabled {
    color: var(--text-muted);
    border-color: var(--border);
    cursor: not-allowed;
  }
  .primary:disabled {
    background: var(--surface-2);
  }
</style>
