<script lang="ts">
  import { t } from '../../i18n';
  import Button from '../../components/Button/Button.svelte';
  import Dialog from '../../components/Dialog/Dialog.svelte';

  interface Props {
    open: boolean;
    onclose: () => void;
  }

  let { open, onclose }: Props = $props();

  const rows = [
    ['Ctrl+K', t('shortcuts.palette')],
    ['Ctrl+O', t('shortcuts.open')],
    ['Enter', t('shortcuts.run')],
    ['Esc', t('shortcuts.escape')],
    ['F1', t('shortcuts.help')],
  ] as const;
</script>

<Dialog {open} title={t('shortcuts.title')} {onclose}>
  <table>
    <tbody>
      {#each rows as [keys, text] (keys)}
        <tr><th scope="row"><kbd>{keys}</kbd></th><td>{text}</td></tr>
      {/each}
    </tbody>
  </table>
  <div class="actions"><Button onclick={onclose}>{t('shortcuts.close')}</Button></div>
</Dialog>

<style>
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th,
  td {
    padding: var(--space-2) var(--space-3);
    text-align: left;
    border-bottom: 1px solid var(--border);
  }
  th {
    width: 1%;
    white-space: nowrap;
  }
  kbd {
    padding: 0 var(--space-2);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--surface-2);
  }
  .actions {
    margin-top: var(--space-4);
    display: flex;
    justify-content: end;
  }
</style>
