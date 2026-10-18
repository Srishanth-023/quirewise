import { ipc, toAppError, type Settings } from '../ipc/client';
import { t } from '../i18n';
import { toasts } from './toasts.svelte';

type Theme = NonNullable<Settings['theme']>;

/** Applies the theme to the document. `system` removes the override so Windows decides. */
export function applyTheme(theme: Theme, root: HTMLElement = document.documentElement): void {
  if (theme === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', theme);
}

class SettingsStore {
  value = $state<Settings | null>(null);

  async load(): Promise<void> {
    try {
      this.value = await ipc.getSettings();
      applyTheme(this.value.theme ?? 'system');
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  /** Saves a change and applies it. The previous value is restored when saving fails. */
  async update(change: Partial<Settings>): Promise<void> {
    const previous = this.value;
    if (!previous) return;
    const next = { ...previous, ...change };
    this.value = next;
    try {
      await ipc.saveSettings(next);
      applyTheme(next.theme ?? 'system');
      toasts.info(t('settings.saved'));
    } catch (error) {
      this.value = previous;
      toasts.error(toAppError(error));
    }
  }
}

export const settings = new SettingsStore();
