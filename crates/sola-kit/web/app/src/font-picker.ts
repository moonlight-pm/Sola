// In-window font picker. Shows only families discovered by Pango at
// startup (passed from KitApp via initial_state.fonts) — no generic
// stacks, no fallbacks. One family selected per token. Each option in
// the list renders its own name in that font.
//
// Single instance open at a time, anchored to its trigger button.
// Outside-click closes.

import { component, html, reactive } from '@arrow-js/core';

interface FontPickerOpts {
  id: string;
  value: () => string;
  options: () => string[];
  onChange: (newValue: string) => void;
  /** Chip-style trigger: borderless, no chevron, sits inline. Stable per instance. */
  compact?: boolean;
}

const local = reactive<{ openId: string | null; query: string }>({ openId: null, query: '' });

document.addEventListener('click', (e) => {
  if (local.openId === null) return;
  const path = (e.composedPath ? e.composedPath() : []) as EventTarget[];
  const inside = path.some((n) => {
    const el = n as HTMLElement;
    return el.classList && el.classList.contains('kit-font-trigger-wrap');
  });
  if (!inside) {
    local.openId = null;
    local.query = '';
  }
});

interface PanelProps {
  options: () => string[];
  value: () => string;
  choose: (name: string) => void;
}

const fontPanel = component((props: PanelProps) => {
  const matches = () => {
    const q = local.query.toLowerCase().trim();
    const all = props.options();
    if (!q) return all;
    return all.filter(name => name.toLowerCase().includes(q));
  };
  return html`<div class="kit-font-popover" @click="${(e: Event) => e.stopPropagation()}">
    <input class="kit-field kit-font-search" placeholder="Search…"
      value="${() => local.query}"
      @input="${(e: Event) => { local.query = (e.target as HTMLInputElement).value; }}">
    <div class="kit-font-list">
      ${() => matches().length === 0
        ? html`<div class="kit-font-empty">No matching fonts</div>`
        : matches().map(name => html`<button
            class="kit-font-option"
            data-active="${() => props.value() === name ? 'active' : false}"
            style="${() => `font-family: ${name}`}"
            @click="${() => props.choose(name)}">${name}</button>`)
      }
    </div>
  </div>`;
});

export const fontPicker = component((props: FontPickerOpts) => {
  const isOpen = () => local.openId === props.id;
  const onTrigger = (e: Event) => {
    e.stopPropagation();
    if (isOpen()) {
      local.openId = null;
      local.query = '';
    } else {
      local.openId = props.id;
      local.query = '';
    }
  };
  const choose = (name: string) => {
    props.onChange(name);
    local.openId = null;
    local.query = '';
  };

  // `compact` is read once at instance init — the wrapper element type
  // (span vs div) can't switch without a remount, so we treat it as stable.
  if (props.compact) {
    return html`<span class="kit-font-trigger-wrap kit-font-trigger-wrap-compact">
      <span class="kit-font-trigger-compact" @click="${onTrigger}"
        style="${() => `font-family: ${props.value()}`}">${() => props.value() || '— select —'}</span>
      ${() => isOpen() ? fontPanel({ options: props.options, value: props.value, choose }) : null}
    </span>`;
  }
  return html`<div class="kit-font-trigger-wrap">
    <button class="kit-font-trigger" @click="${onTrigger}"
      style="${() => `font-family: ${props.value()}`}">
      <span class="kit-font-trigger-name">${() => props.value() || '— select —'}</span>
      <span class="kit-font-trigger-chev">▾</span>
    </button>
    ${() => isOpen() ? fontPanel({ options: props.options, value: props.value, choose }) : null}
  </div>`;
});
