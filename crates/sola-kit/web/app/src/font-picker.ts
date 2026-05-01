// In-window font picker. Shows only families discovered by Pango at
// startup (passed from KitApp via initial_state.fonts) — no generic
// stacks, no fallbacks. One family selected per token. Each option in
// the list renders its own name in that font.
//
// Single instance open at a time, anchored to its trigger button.
// Outside-click closes.

import { html, reactive } from '@arrow-js/core';

interface FontPickerOpts {
  id: string;
  value: () => string;
  options: () => string[];
  onChange: (newValue: string) => void;
}

const local = reactive<{ openId: string | null; query: string }>({ openId: null, query: '' });

document.addEventListener('click', (e) => {
  if (local.openId === null) return;
  const path = (e.composedPath ? e.composedPath() : []) as EventTarget[];
  const inside = path.some((n) => {
    const el = n as HTMLElement;
    return el.classList && (
      el.classList.contains('kit-font-trigger') ||
      el.classList.contains('kit-font-popover')
    );
  });
  if (!inside) {
    local.openId = null;
    local.query = '';
  }
});

export function fontPicker(opts: FontPickerOpts) {
  const isOpen = () => local.openId === opts.id;
  const onTrigger = (e: Event) => {
    e.stopPropagation();
    if (isOpen()) {
      local.openId = null;
      local.query = '';
    } else {
      local.openId = opts.id;
      local.query = '';
    }
  };
  return html`<div class="kit-font-trigger-wrap">
    <button class="kit-font-trigger" @click="${onTrigger}"
      style="${() => `font-family: ${opts.value()}`}">
      <span class="kit-font-trigger-name">${() => opts.value() || '— select —'}</span>
      <span class="kit-font-trigger-chev">▾</span>
    </button>
    ${() => isOpen() ? renderPanel(opts) : html``}
  </div>`;
}

function renderPanel(opts: FontPickerOpts) {
  const matches = () => {
    const q = local.query.toLowerCase().trim();
    const all = opts.options();
    if (!q) return all;
    return all.filter(name => name.toLowerCase().includes(q));
  };
  const choose = (name: string) => {
    opts.onChange(name);
    local.openId = null;
    local.query = '';
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
            data-active="${() => opts.value() === name ? 'active' : false}"
            style="${() => `font-family: ${name}`}"
            @click="${() => choose(name)}">${name}</button>`)
      }
    </div>
  </div>`;
}
