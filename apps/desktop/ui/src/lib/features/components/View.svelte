<script lang="ts">
  import { onMount } from 'svelte';
  import { t, tDynamic } from '../../i18n';
  import Dialog from '../../components/Dialog/Dialog.svelte';
  import ProgressBar from '../../components/ProgressBar/ProgressBar.svelte';
  import { components, componentName } from './store.svelte';

  onMount(() => void components.load());

  const mb = (bytes: number | bigint): number => Math.max(1, Math.round(Number(bytes) / (1024 * 1024)));

  const question = $derived.by(() => {
    const c = components.consent;
    if (!c) return '';
    return c.kind === 'check'
      ? t('components.consent.check')
      : t('components.consent.install', {
          name: componentName(c.entry.id),
          version: c.entry.version,
          mb: mb(c.entry.size),
        });
  });
</script>

<section class="components" aria-labelledby="components-title">
  <h2 id="components-title">{t('components.title')}</h2>
  <p class="help">{t('components.intro')}</p>

  <h3>{t('components.installed')}</h3>
  {#if components.installed.length === 0}
    <p class="help">{t('components.none')}</p>
  {:else}
    <ul>
      {#each components.installed as entry (entry.id + entry.version)}
        <li>
          <div>
            <strong>{componentName(entry.id)}</strong>
            <span class="mono">{t('components.version', { version: entry.version })}</span>
            <span class="mono">{t('components.size', { mb: mb(entry.size) })}</span>
            <p class="help">{tDynamic(`component.${entry.id}.use`, '')}</p>
          </div>
          <button
            type="button"
            aria-label={t('components.remove', { name: componentName(entry.id) })}
            onclick={() => void components.remove(entry)}>{t('option.remove')}</button
          >
        </li>
      {/each}
    </ul>
  {/if}

  <h3>{t('components.available')}</h3>
  <div class="row">
    <button type="button" disabled={components.checking || components.busy !== null} onclick={() => components.ask({ kind: 'check' })}>
      {components.checking ? t('components.checking') : t('components.check')}
    </button>
    <button type="button" disabled={components.busy !== null} onclick={() => void components.importPackage()}>
      {t('components.import')}
    </button>
  </div>
  {#if components.available !== null}
    {#if components.installable.length === 0}
      <p class="help">{t('components.noneAvailable')}</p>
    {:else}
      <ul>
        {#each components.installable as entry (entry.id + entry.version)}
          <li>
            <div>
              <strong>{componentName(entry.id)}</strong>
              <span class="mono">{t('components.version', { version: entry.version })}</span>
              <span class="mono">{t('components.size', { mb: mb(entry.size) })}</span>
              <p class="help">{tDynamic(`component.${entry.id}.use`, '')}</p>
              {#if components.busy === entry.id}
                <ProgressBar
                  current={components.progress.done}
                  total={components.progress.total}
                  label={t('components.installing', { name: componentName(entry.id) })}
                />
              {/if}
            </div>
            <button
              type="button"
              disabled={components.busy !== null}
              aria-label={t('components.install', { name: componentName(entry.id) })}
              onclick={() => components.ask({ kind: 'install', entry })}>{t('components.installButton')}</button
            >
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<Dialog open={components.consent !== null} title={t('components.consent.title')} onclose={() => components.decline()}>
  <p>{question}</p>
  <div class="row">
    <button type="button" class="primary" onclick={() => void components.confirm()}>{t('components.consent.yes')}</button>
    <button type="button" onclick={() => components.decline()}>{t('components.consent.no')}</button>
  </div>
</Dialog>

<style>
  .components {
    margin-top: var(--space-7);
    max-width: 70ch;
  }
  h2 {
    margin: 0 0 var(--space-3);
  }
  h3 {
    margin: var(--space-5) 0 var(--space-3);
    font-size: var(--text-base);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  li {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
  }
  li > div {
    flex: 1;
    min-width: 0;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .help {
    margin: var(--space-2) 0 0;
    color: var(--text-muted);
  }
  button {
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  button.primary {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 500;
  }
  button:disabled {
    color: var(--text-muted);
  }
</style>
