<script lang="ts">
  import { t } from '../../lib/i18n';
  import Button from '../../lib/components/Button/Button.svelte';
  import Dialog from '../../lib/components/Dialog/Dialog.svelte';

  interface Props {
    open: boolean;
    onclose: () => void;
    onconfirm: () => void;
  }

  let { open, onclose, onconfirm }: Props = $props();
</script>

<!-- Cancelling a running job is the one action that asks first: it cannot be undone. -->
<Dialog {open} title={t('cancel.title')} {onclose}>
  <p>{t('cancel.body')}</p>
  <div class="actions">
    <Button onclick={onclose}>{t('cancel.no')}</Button>
    <Button variant="primary" onclick={onconfirm}>{t('cancel.yes')}</Button>
  </div>
</Dialog>

<style>
  .actions {
    display: flex;
    justify-content: end;
    gap: var(--space-3);
  }
</style>
