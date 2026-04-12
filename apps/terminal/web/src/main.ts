import './theme.css';
import '@xterm/xterm/css/xterm.css';
import { createApp } from './app';

createApp(document.getElementById('app')!).catch((e) => {
  document.title = 'app-error:' + String(e);
  console.error('[sola-terminal] createApp failed:', e);
});
