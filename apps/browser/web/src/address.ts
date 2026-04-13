import { html } from '@arrow-js/core';

export interface AddressBarConfig {
  value: () => string;
  suggestions: () => Array<{ url: string; title: string; visits: number }>;
  onNavigate: (input: string) => void;
  onInput: (value: string) => void;
  onFocus: () => void;
  onBlur: () => void;
}

export function createAddressBar(config: AddressBarConfig, target: HTMLElement): void {
  let debounceTimer: number | null = null;

  function onInput(e: Event): void {
    const value = (e.target as HTMLInputElement).value;
    config.onInput(value);
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => config.onInput(value), 150) as unknown as number;
  }

  function onKeyDown(e: KeyboardEvent): void {
    if (e.key === 'Enter') {
      e.preventDefault();
      config.onNavigate(config.value());
      (e.target as HTMLElement).blur();
    } else if (e.key === 'Escape') {
      (e.target as HTMLElement).blur();
    }
  }

  function selectSuggestion(url: string): void {
    config.onNavigate(url);
  }

  function onFocus(e: Event): void {
    (e.target as HTMLInputElement).select();
    config.onFocus();
  }

  function onBlur(): void {
    setTimeout(() => config.onBlur(), 200);
  }

  html`
    <div class="address-bar">
      <input
        class="address-input"
        type="text"
        placeholder="Search or enter URL"
        value="${config.value}"
        @input="${onInput}"
        @keydown="${onKeyDown}"
        @focus="${onFocus}"
        @blur="${onBlur}"
      />
      ${() => config.suggestions().length > 0 ? html`
        <div class="autocomplete-list">
          ${() => config.suggestions().map(s =>
            html`<div class="autocomplete-item" @mousedown="${() => selectSuggestion(s.url)}">
              <span class="autocomplete-item-title">${() => s.title}</span>
              <span class="autocomplete-item-url">${() => s.url}</span>
            </div>`
          )}
        </div>
      ` : html``}
    </div>
  `(target);
}
