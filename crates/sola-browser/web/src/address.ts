import { html, reactive, watch } from '@arrow-js/core';

export interface TopBarConfig {
  value: () => string;
  suggestions: () => Array<{ url: string; title: string; visits: number }>;
  /** True when the browser has an active tab; when false the address input is disabled. */
  enabled: () => boolean;
  /** Bump this value to request address-input focus. Reactive watch fires on change. */
  focusNonce: () => number;
  onBack: () => void;
  onForward: () => void;
  onReload: () => void;
  onNavigate: (input: string) => void;
  onInput: (value: string) => void;
  onBlur: () => void;
}

const local = reactive({ copied: false });
let copyTimer: number | null = null;

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

  // When the input is already focused, clicking it normally places the
  // caret at the click point. Match browser-address-bar convention: if
  // the user has not actively selected a range, select all on click.
  function onClick(e: MouseEvent) {
    const input = e.target as HTMLInputElement;
    if (input.selectionStart === input.selectionEnd) {
      input.select();
    }
  }

  function onBlur() {
    setTimeout(() => config.onBlur(), 200);
  }

  function onCopyUrl() {
    const url = config.value();
    if (!url) return;
    navigator.clipboard.writeText(url).then(() => {
      local.copied = true;
      if (copyTimer != null) clearTimeout(copyTimer);
      copyTimer = window.setTimeout(() => { local.copied = false; copyTimer = null; }, 1200);
    });
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
          @click="${onClick}"
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
      <button class="nav-btn copy-url-btn tooltip-end"
        data-tooltip="${() => local.copied ? 'Copied!' : 'Copy URL'}"
        @mousedown="${(e: MouseEvent) => e.preventDefault()}"
        @click="${onCopyUrl}"
      ><span class="${() => local.copied ? 'icon icon-check' : 'icon icon-copy'}"></span></button>
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

  // Reactive disabled state: HTML boolean attrs can't be toggled by
  // Arrow.js string expressions (presence/absence matters, not value),
  // so drive the .disabled DOM property imperatively from a reactive
  // watch. Arrow.js's watch() is the sanctioned side-effect primitive.
  watch(() => {
    const disabled = !config.enabled();
    const input = target.querySelector<HTMLInputElement>('.address-input');
    if (input) input.disabled = disabled;
  });
}
