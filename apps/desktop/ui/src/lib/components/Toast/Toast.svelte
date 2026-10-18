<script lang="ts">
  import { t } from '../../i18n';
  import { toasts } from '../../stores/toasts.svelte';
  import Icon from '../Icon.svelte';
</script>

<!-- A polite live region: screen readers hear new messages without focus moving. -->
<div class="region" role="status" aria-live="polite">
  {#each toasts.items as toast (toast.id)}
    <div class="toast" class:error={toast.kind === 'error'}>
      <p>{toast.text}</p>
      <button type="button" aria-label={t('toast.close')} onclick={() => toasts.dismiss(toast.id)}>
        <Icon name="x" />
      </button>
    </div>
  {/each}
</div>

<style>
  .region {
    position: fixed;
    right: var(--space-5);
    bottom: var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: min(420px, calc(100vw - var(--space-7)));
    z-index: 10;
  }
  .toast {
    display: flex;
    align-items: start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-panel);
    background: var(--surface);
  }
  .error {
    border-color: var(--danger);
  }
  p {
    margin: 0;
    flex: 1;
  }
  button {
    min-width: var(--target-min);
    min-height: var(--target-min);
    border: 0;
    background: transparent;
    display: grid;
    place-items: center;
  }
</style>
