// <kit-font-picker .options=${[...]} value="Sans"></kit-font-picker>
//   or compact form: <kit-font-picker compact ...></kit-font-picker>
//
// Searchable popover of installed font families. Each option renders
// in its own font for instant recognition. Dispatches `kit-font-change`
// (detail: { value }) on selection.

import { html } from 'lit-html';
import { signal } from 'signals';
import { KitElement } from '@sola/kit';

const openId = signal<string | null>(null);
const query = signal<string>('');

document.addEventListener('click', (e) => {
  if (openId.value === null) return;
  const path = (e.composedPath ? e.composedPath() : []) as EventTarget[];
  const inside = path.some((n) => (n as HTMLElement).tagName === 'KIT-FONT-PICKER');
  if (!inside) {
    openId.value = null;
    query.value = '';
  }
});

let nextId = 1;

export class KitFontPicker extends KitElement {
  static properties = {
    value: { type: String },
    options: { type: Array },
    compact: { type: Boolean },
  };
  declare value: string;
  declare options: string[];
  declare compact?: boolean;

  #id = `font:${nextId++}`;

  render() {
    const isOpen = openId.value === this.#id;
    const display = this.value || '— select —';
    if (this.compact) {
      return html`<span class="kit-font-trigger-wrap kit-font-trigger-wrap-compact">
        <span class="kit-font-trigger-compact" @click=${this.#onTrigger}
          style=${`font-family: ${this.value ?? ''}`}>${display}</span>
        ${isOpen ? this.#panel() : ''}
      </span>`;
    }
    return html`<div class="kit-font-trigger-wrap">
      <button class="kit-font-trigger" @click=${this.#onTrigger}
        style=${`font-family: ${this.value ?? ''}`}>
        <span class="kit-font-trigger-name">${display}</span>
        <span class="kit-font-trigger-chev">▾</span>
      </button>
      ${isOpen ? this.#panel() : ''}
    </div>`;
  }

  #onTrigger = (e: Event) => {
    e.stopPropagation();
    if (openId.value === this.#id) {
      openId.value = null;
      query.value = '';
    } else {
      openId.value = this.#id;
      query.value = '';
    }
  };

  #choose(name: string) {
    this.dispatchEvent(new CustomEvent('kit-font-change', { detail: { value: name }, bubbles: true }));
    openId.value = null;
    query.value = '';
  }

  #panel() {
    const q = query.value.toLowerCase().trim();
    const all = this.options ?? [];
    const matches = q ? all.filter(name => name.toLowerCase().includes(q)) : all;
    return html`<div class="kit-font-popover" @click=${(e: Event) => e.stopPropagation()}>
      <input class="kit-field kit-font-search" placeholder="Search…"
        .value=${query.value}
        @input=${(e: Event) => { query.value = (e.target as HTMLInputElement).value; }}>
      <div class="kit-font-list">
        ${matches.length === 0
          ? html`<div class="kit-font-empty">No matching fonts</div>`
          : matches.map(name => html`<button
              class="kit-font-option"
              data-active=${this.value === name ? 'active' : ''}
              style=${`font-family: ${name}`}
              @click=${() => this.#choose(name)}>${name}</button>`)
        }
      </div>
    </div>`;
  }
}

customElements.define('kit-font-picker', KitFontPicker);
