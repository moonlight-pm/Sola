import { html, watch } from '@arrow-js/core';

export interface TopBarConfig {
  value: () => string;
  suggestions: () => Array<{ url: string; title: string; visits: number }>;
  /** Bump this value to request address-input focus. Reactive watch fires on change. */
  focusNonce: () => number;
  onBack: () => void;
  onForward: () => void;
  onReload: () => void;
  onNavigate: (input: string) => void;
  onInput: (value: string) => void;
  onBlur: () => void;
}

export function createTopBar(config: TopBarConfig, target: HTMLElement): void {
  function onInput(e: Event) {
    config.onInput((e.target as HTMLInputElement).value);
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      config.onNavigate(config.value());
      (e.target as HTMLElement).blur();
    } else if (e.key === 'Escape') {
      (e.target as HTMLElement).blur();
    }
  }

  function onFocus(e: Event) {
    (e.target as HTMLInputElement).select();
  }

  function onBlur() {
    setTimeout(() => config.onBlur(), 200);
  }

  html`
    <div class="top-bar">
      <button class="nav-btn" @click="${config.onBack}"><span class="icon icon-arrow-left"></span></button>
      <button class="nav-btn" @click="${config.onForward}"><span class="icon icon-arrow-right"></span></button>
      <button class="nav-btn" @click="${config.onReload}"><span class="icon icon-rotate-cw"></span></button>
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
    </div>
  `(target);

  // Reactive focus: when focusNonce changes, focus the address input.
  // Scoped target.querySelector, run on the next frame so the template
  // is laid out before we grab focus.
  let last = config.focusNonce();
  watch(() => {
    const n = config.focusNonce();
    if (n === last) return;
    last = n;
    requestAnimationFrame(() => {
      target.querySelector<HTMLInputElement>('.address-input')?.focus();
    });
  });
}
