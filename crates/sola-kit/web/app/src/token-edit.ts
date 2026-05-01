import { html, reactive } from '@arrow-js/core';
import { applyTheme, button } from '@sola/kit';
import { invoke, on } from '@sola/ipc';
import type { CatalogEntry } from './sidebar';

// Holds the in-progress full Theme as JSON, mirroring the Rust struct
// passed in via initial_state. Mutations propagate to applyTheme(...)
// immediately (live preview) and emit Topic::Theme via debounced bus
// after 300 ms of inactivity.
export const themeState = reactive({
  // shape: { colors: {...}, typography: {...}, spacing: {...}, radius: {...} }
  current: (window as unknown as { RESTORED_STATE?: { theme: any } }).RESTORED_STATE?.theme ?? {},
});

// KitApp pushes 'theme_state' alongside the framework's 'theme' event so
// our local mirror stays in sync with the persisted Theme on reset / peer
// updates. Without this, the swatches and color inputs would lag behind
// :root after a theme_reset or sticky replay.
on('theme_state', (payload: { theme: unknown }) => {
  if (payload && payload.theme) themeState.current = payload.theme;
});

let debounceTimer: number | null = null;

export function setColor(field: string, value: string) {
  if (!themeState.current.colors) return;
  themeState.current.colors[field] = value;
  applyTheme({ [`--${field.replaceAll('_', '-')}`]: value });
  scheduleEmit();
}

export function setTypography(field: string, value: string) {
  if (!themeState.current.typography) return;
  themeState.current.typography[field] = value;
  applyTheme({ [`--${field.replaceAll('_', '-')}`]: value });
  scheduleEmit();
}

export function setSpacing(field: string, value: string) {
  if (!themeState.current.spacing) return;
  themeState.current.spacing[field] = value;
  applyTheme({ [`--space-${field}`]: value });
  scheduleEmit();
}

export function setRadius(field: string, value: string) {
  if (!themeState.current.radius) return;
  themeState.current.radius[field] = value;
  applyTheme({ [`--radius-${field}`]: value });
  scheduleEmit();
}

function scheduleEmit() {
  if (debounceTimer !== null) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    invoke('theme_set', { theme: themeState.current });
  }, 300) as unknown as number;
}

export function resetTheme() {
  // Server-side default is authoritative. Clear local copy + ask
  // KitApp to re-emit Theme::default() by sending an empty hint;
  // KitApp's sticky replay will then push the new (default) state back
  // and our themeState will refresh via the theme listener.
  invoke('theme_reset', {});
}

/** Editor strip for one color token. */
export function colorEditor(field: string, varName: string, used: CatalogEntry[]) {
  const current = (): string => themeState.current?.colors?.[field] ?? '';
  return html`
    <div class="kit-editor-strip">
      <div class="kit-editor-head">
        <div class="kit-editor-name">${varName}</div>
        <div class="kit-editor-meta">${() => `Used in ${used.length} ${used.length === 1 ? 'component' : 'components'}`}</div>
      </div>
      <div class="kit-editor-row">
        <div class="kit-editor-swatch" style="${() => `background: ${current()}`}"></div>
        <input type="text" class="kit-field" value="${current}" @input=${(e: Event) => setColor(field, (e.target as HTMLInputElement).value)}>
        <input type="color" value="${() => normaliseToHex(current())}" @input=${(e: Event) => setColor(field, (e.target as HTMLInputElement).value)}>
        <div class="kit-editor-actions">${button({ label: 'Reset', variant: 'ghost', onClick: resetTheme })}</div>
      </div>
    </div>
  `;
}

/** Best-effort hex form for the <input type="color"> element. */
function normaliseToHex(value: string): string {
  if (value.startsWith('#') && (value.length === 7 || value.length === 4)) return value;
  // For rgba(...) / non-hex, fall back to a neutral so the picker isn't
  // broken; the text input is still authoritative.
  return '#000000';
}
