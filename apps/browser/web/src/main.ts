// Debug: log all messages from Rust before @sola/ipc processes them
const origRecv = (window as any).__solaRecv;
(window as any).__solaRecv = (json: string) => {
  console.log('[browser] __solaRecv raw:', json.substring(0, 200));
  if (origRecv) origRecv(json);
};

import { createApp } from './app.js';

createApp(document.getElementById('app')!).catch((e) => {
  document.title = 'app-error:' + String(e);
  console.error('[sola-browser] createApp failed:', e);
});
