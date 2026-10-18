import type { AppError } from '../ipc/client';

/** A short message shown in the live region. */
export interface Toast {
  id: number;
  kind: 'info' | 'error';
  text: string;
}

const AUTO_DISMISS_MS = 8000;

class Toasts {
  items = $state<Toast[]>([]);
  #next = 1;

  /** Shows an informational message. */
  info(text: string): void {
    this.#push('info', text);
  }

  /** Shows an error. The message already says what happened and what to do next. */
  error(error: AppError): void {
    this.#push('error', `${error.message} (${error.code})`);
  }

  dismiss(id: number): void {
    this.items = this.items.filter((t) => t.id !== id);
  }

  #push(kind: Toast['kind'], text: string): void {
    const id = this.#next++;
    this.items = [...this.items, { id, kind, text }];
    setTimeout(() => this.dismiss(id), AUTO_DISMISS_MS);
  }
}

export const toasts = new Toasts();
