import { cleanup } from '@testing-library/svelte';
import { afterEach } from 'vitest';

// Testing Library only cleans up automatically when test globals are on; they are off here.
afterEach(() => cleanup());

// jsdom has no modal dialog support. WebView2 does, so this only fills the gap for tests.
if (typeof HTMLDialogElement !== 'undefined') {
  HTMLDialogElement.prototype.showModal ??= function showModal(this: HTMLDialogElement) {
    this.setAttribute('open', '');
  };
  HTMLDialogElement.prototype.close ??= function close(this: HTMLDialogElement) {
    this.removeAttribute('open');
    this.dispatchEvent(new Event('close'));
  };
}
