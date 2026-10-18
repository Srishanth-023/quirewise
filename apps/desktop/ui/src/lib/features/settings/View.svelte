<script lang="ts">
  import { t } from '../../i18n';
  import type { Settings } from '../../ipc/client';
  import { settings } from '../../stores/settings.svelte';

  const themes = ['system', 'light', 'dark'] as const;
  const themeLabel = {
    system: t('settings.theme.system'),
    light: t('settings.theme.light'),
    dark: t('settings.theme.dark'),
  };

  function onTheme(event: Event): void {
    const value = (event.currentTarget as HTMLSelectElement).value as NonNullable<Settings['theme']>;
    void settings.update({ theme: value });
  }

  function onRecent(event: Event): void {
    void settings.update({ remember_recent_files: (event.currentTarget as HTMLInputElement).checked });
  }
</script>

<section aria-labelledby="settings-title">
  <h1 id="settings-title">{t('settings.title')}</h1>
  {#if settings.value}
    <div class="field">
      <label for="theme">{t('settings.theme')}</label>
      <select id="theme" value={settings.value.theme ?? 'system'} onchange={onTheme}>
        {#each themes as theme (theme)}
          <option value={theme}>{themeLabel[theme]}</option>
        {/each}
      </select>
    </div>
    <div class="field">
      <label class="check">
        <input type="checkbox" checked={settings.value.remember_recent_files ?? true} onchange={onRecent} />
        {t('settings.recent')}
      </label>
      <p class="help">{t('settings.recentHelp')}</p>
    </div>
  {/if}
</section>

<style>
  .field {
    margin-bottom: var(--space-5);
    max-width: 60ch;
  }
  label {
    display: block;
    margin-bottom: var(--space-2);
    font-weight: 500;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--target-min);
  }
  select {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .help {
    margin: var(--space-2) 0 0;
    color: var(--text-muted);
  }
</style>
