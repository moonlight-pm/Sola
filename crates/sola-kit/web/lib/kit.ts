//! Sola Kit — design tokens, atoms, components.
//
// Importing this module installs a bus listener that applies any
// Topic::Theme broadcasts the framework forwards. Apps don't need to
// opt in beyond the import.

import { on } from '@sola/ipc';

/** Apply a map of CSS custom properties to :root. */
export function applyTheme(vars: Record<string, string>): void {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(vars)) {
    root.style.setProperty(key.startsWith('--') ? key : `--${key}`, value);
  }
}

// Self-install: framework forwards Topic::Theme as { event: 'theme', vars }.
on('theme', (payload: { vars: Record<string, string> }) => {
  if (payload && payload.vars) applyTheme(payload.vars);
});
