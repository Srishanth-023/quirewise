import { mount } from 'svelte';
import './lib/design/tokens.css';
import './lib/design/reset.css';
import './lib/design/typography.css';
import App from './app/App.svelte';

// In a plain browser during UI work there is no Tauri; a development-only mock stands in. The
// condition is a compile-time constant, so this import is removed from production builds.
if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
  const { installMockTauri } = await import('./dev/mockTauri');
  installMockTauri();
}

const target = document.getElementById('app');
if (!target) {
  throw new Error('Root element #app is missing from index.html');
}

export default mount(App, { target });
