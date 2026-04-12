import { createApp } from './app.js';

createApp(document.getElementById('app')!).catch((e) => {
  document.title = 'app-error:' + String(e);
  console.error('[sola-terminal] createApp failed:', e);
});
