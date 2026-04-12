import { html } from '@arrow-js/core';
import { state, navigate, searchHistory } from './app.js';

let debounceTimer: number | null = null;

function onInput(e: Event): void {
  state.addressValue = (e.target as HTMLInputElement).value;
  if (debounceTimer !== null) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => searchHistory(state.addressValue), 150) as unknown as number;
}

function onKeyDown(e: KeyboardEvent): void {
  if (e.key === 'Enter') {
    e.preventDefault();
    state.suggestions = [];
    navigate(state.addressValue);
    (e.target as HTMLElement).blur();
  } else if (e.key === 'Escape') {
    state.suggestions = [];
    (e.target as HTMLElement).blur();
  }
}

function selectSuggestion(url: string): void {
  state.addressValue = url;
  state.suggestions = [];
  navigate(url);
}

function onFocus(e: Event): void {
  (e.target as HTMLInputElement).select();
  state.addressFocused = true;
}

function onBlur(): void {
  setTimeout(() => {
    state.addressFocused = false;
    state.suggestions = [];
  }, 200);
}

export function renderAddressBar(): any {
  return html`
    <div class="address-bar">
      <input
        class="address-input"
        type="text"
        placeholder="Search or enter URL"
        value="${() => state.addressValue}"
        @input="${onInput}"
        @keydown="${onKeyDown}"
        @focus="${onFocus}"
        @blur="${onBlur}"
      />
      ${() => state.suggestions.length > 0 ? html`
        <div class="autocomplete-list">
          ${() => state.suggestions.map(s =>
            html`<div class="autocomplete-item" @mousedown="${() => selectSuggestion(s.url)}">
              <span class="autocomplete-item-title">${() => s.title}</span>
              <span class="autocomplete-item-url">${() => s.url}</span>
            </div>`
          )}
        </div>
      ` : html``}
    </div>
  `;
}
