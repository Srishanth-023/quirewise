<script lang="ts">
  import { t } from '../../i18n';
  import Button from '../../components/Button/Button.svelte';
  import { files, type FileEntry } from '../../stores/files.svelte';

  interface Props {
    entry: FileEntry;
  }

  let { entry }: Props = $props();

  let password = $state('');
  let busy = $state(false);

  const wrong = $derived(entry.problem?.code === 'QW-PDF-WRONG-PASSWORD');

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!password || busy) return;
    busy = true;
    await files.unlock(entry.id, password);
    busy = false;
    password = '';
  }
</script>

<!-- Inline, once per file: no dialog. The password stays in memory for this session only. -->
<form onsubmit={submit} aria-labelledby={`pw-title-${entry.id}`}>
  <h3 id={`pw-title-${entry.id}`}>{t('password.title')}</h3>
  <label for={`pw-${entry.id}`}>{t('password.label', { name: entry.name })}</label>
  <div class="row">
    <input
      id={`pw-${entry.id}`}
      type="password"
      autocomplete="off"
      bind:value={password}
      aria-invalid={wrong}
      aria-describedby={`pw-hint-${entry.id}`}
    />
    <Button type="submit" variant="primary" disabled={!password || busy}>
      {t('password.unlock', { name: entry.name })}
    </Button>
  </div>
  <p id={`pw-hint-${entry.id}`} class:error={wrong}>
    {wrong ? t('password.wrong') : t('password.hint')}
  </p>
</form>

<style>
  form {
    padding: var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-panel);
    background: var(--surface);
  }
  h3 {
    margin: 0 0 var(--space-3);
  }
  label {
    display: block;
    margin-bottom: var(--space-2);
  }
  .row {
    display: flex;
    gap: var(--space-3);
  }
  input {
    flex: 1;
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
  }
  input[aria-invalid='true'] {
    border-color: var(--danger);
  }
  p {
    margin: var(--space-3) 0 0;
    color: var(--text-muted);
  }
  .error {
    color: var(--danger);
  }
</style>
