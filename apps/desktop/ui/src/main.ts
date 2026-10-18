import { mount } from 'svelte';
import './lib/design/tokens.css';
import './lib/design/reset.css';
import './lib/design/typography.css';
import App from './app/App.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('Root element #app is missing from index.html');
}

export default mount(App, { target });
