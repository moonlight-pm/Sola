import { html } from '@arrow-js/core';

export interface AddressBarConfig {
  value: () => string;
  suggestions: () => Array<{ url: string; title: string; visits: number }>;
  onNavigate: (input: string) => void;
  onInput: (value: string) => void;
  onBlur: () => void;
}

export function createAddressBar(config: AddressBarConfig, target: HTMLElement): void {
  function onInput(e: Event): void {
    config.onInput((e.target as HTMLInputElement).value);
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

  function onFocus(e: Event): void {
    (e.target as HTMLInputElement).select();
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
            html`<div class="autocomplete-item" @mousedown="${() => config.onNavigate(s.url)}">
              <span class="autocomplete-item-title">${() => s.title}</span>
              <span class="autocomplete-item-url">${() => s.url}</span>
            </div>`
          )}
        </div>
      ` : html``}
    </div>
  `(target);
}
