import { html } from './arrow.js';
import { state, navigate, searchHistory } from './app.js';

let debounceTimer = null;

function onInput(e) {
  state.addressValue = e.target.value;
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => searchHistory(state.addressValue), 150);
}

function onKeyDown(e) {
  if (e.key === 'Enter') {
    e.preventDefault();
    state.suggestions = [];
    navigate(state.addressValue);
    e.target.blur();
  } else if (e.key === 'Escape') {
    state.suggestions = [];
    e.target.blur();
  }
}

function selectSuggestion(url) {
  state.addressValue = url;
  state.suggestions = [];
  navigate(url);
}

function onFocus(e) {
  e.target.select();
  state.addressFocused = true;
}

function onBlur() {
  setTimeout(() => {
    state.addressFocused = false;
    state.suggestions = [];
  }, 200);
}

export function renderAddressBar() {
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
