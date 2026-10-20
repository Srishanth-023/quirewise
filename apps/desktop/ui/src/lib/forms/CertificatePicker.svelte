<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n';
  import { ipc, toAppError, type StoredCertificate } from '../ipc/client';
  import { toasts } from '../stores/toasts.svelte';

  interface Props {
    id: string;
    label: string;
    /** The thumbprint that is chosen, or an empty text. */
    value: string;
    onchange: (value: string) => void;
  }

  let { id, label, value, onchange }: Props = $props();

  /** Certificates that can sign, or null until they are known. */
  let found = $state<StoredCertificate[] | null>(null);

  onMount(async () => {
    try {
      found = await ipc.listCertificates();
      // The only choice is taken for the person: one less thing to do.
      if (found.length === 1 && value === '') onchange(found[0]?.thumbprint ?? '');
    } catch (error) {
      found = [];
      toasts.error(toAppError(error));
    }
  });

  const describe = (c: StoredCertificate): string =>
    t('certificates.entry', { subject: c.subject, issuer: c.issuer, until: c.valid_until });
</script>

<div class="field">
  <label for={id}>{label}</label>
  {#if found === null}
    <p class="hint">{t('certificates.loading')}</p>
  {:else if found.length === 0}
    <p class="hint" id={`${id}-hint`}>{t('certificates.none')}</p>
  {:else}
    <select {id} {value} aria-describedby={`${id}-hint`} onchange={(e) => onchange(e.currentTarget.value)}>
      {#if value === ''}<option value="">{t('certificates.choose')}</option>{/if}
      {#each found as c (c.thumbprint)}
        <option value={c.thumbprint}>{describe(c)}</option>
      {/each}
    </select>
    <p class="hint" id={`${id}-hint`}>{t('certificates.hint')}</p>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  label {
    font-weight: 500;
  }
  select {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
  }
  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
</style>
