import { ipc, onComponentProgress, toAppError } from '../../ipc/client';
import type { ComponentEntry } from '../../ipc/generated';
import { t, tDynamic } from '../../i18n';
import { toasts } from '../../stores/toasts.svelte';

/** What the person has been asked to confirm before the internet is used. */
export type Consent = { kind: 'check' } | { kind: 'install'; entry: ComponentEntry };

/** Display name of a component: translated when known, the id otherwise. */
export function componentName(id: string): string {
  return tDynamic(`component.${id}`, id);
}

/** Installed and installable components, and the work of installing them. */
class Components {
  installed = $state<ComponentEntry[]>([]);
  /** Components the signed list offers, or null until the person asked. */
  available = $state<ComponentEntry[] | null>(null);
  /** The question waiting for a yes before any network use. */
  consent = $state<Consent | null>(null);
  /** Id of the component being installed. */
  busy = $state<string | null>(null);
  checking = $state(false);
  progress = $state({ done: 0, total: 0 });

  async load(): Promise<void> {
    try {
      this.installed = await ipc.componentsInstalled();
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  /** The components offered that are not installed yet in this version. */
  get installable(): ComponentEntry[] {
    return (this.available ?? []).filter(
      (a) => !this.installed.some((i) => i.id === a.id && i.version === a.version),
    );
  }

  ask(consent: Consent): void {
    this.consent = consent;
  }

  /** Runs what the person agreed to. */
  async confirm(): Promise<void> {
    const consent = this.consent;
    this.consent = null;
    if (!consent) return;
    if (consent.kind === 'check') await this.#check();
    else await this.#install(consent.entry);
  }

  decline(): void {
    this.consent = null;
  }

  async #check(): Promise<void> {
    this.checking = true;
    try {
      this.available = await ipc.componentsAvailable(true);
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      this.checking = false;
    }
  }

  async #install(entry: ComponentEntry): Promise<void> {
    this.busy = entry.id;
    this.progress = { done: 0, total: Number(entry.size) };
    const stop = await onComponentProgress((p) => {
      if (p.id === entry.id) this.progress = { done: p.done, total: p.total };
    });
    try {
      const done = await ipc.componentInstall(entry.id, true);
      toasts.info(t('components.installed.toast', { name: componentName(done.id), version: done.version }));
      await this.load();
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      stop();
      this.busy = null;
    }
  }

  /** Installs from a `.qwpkg` file chosen in the system dialog. No network is used. */
  async importPackage(): Promise<void> {
    try {
      const [path] = await ipc.pickPaths('package', false);
      if (!path) return;
      const done = await ipc.componentImport(path);
      toasts.info(t('components.installed.toast', { name: componentName(done.id), version: done.version }));
      await this.load();
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  async remove(entry: ComponentEntry): Promise<void> {
    try {
      await ipc.componentRemove(entry.id);
      toasts.info(t('components.removed', { name: componentName(entry.id) }));
      await this.load();
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }
}

export const components = new Components();
