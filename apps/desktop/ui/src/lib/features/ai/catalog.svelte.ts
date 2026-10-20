import { ipc, toAppError } from '../../ipc/client';
import type { AiCatalog } from '../../ipc/generated';
import { toasts } from '../../stores/toasts.svelte';

/** The models and languages of the local AI tools, asked of the core once. */
class Catalog {
  data = $state<AiCatalog | null>(null);
  #loading: Promise<void> | null = null;

  /** Loads the catalog the first time; calls made while it loads wait for the same answer. */
  load(): Promise<void> {
    if (this.data) return Promise.resolve();
    this.#loading ??= (async () => {
      try {
        this.data = await ipc.aiCatalog();
      } catch (error) {
        toasts.error(toAppError(error));
      } finally {
        this.#loading = null;
      }
    })();
    return this.#loading;
  }
}

export const catalog = new Catalog();
