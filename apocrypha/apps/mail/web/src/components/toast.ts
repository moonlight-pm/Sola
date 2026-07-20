import { html, watch } from '@arrow-js/core';

export interface ToastConfig {
  message: () => string | null;
  onDismiss: () => void;
}

export function createToast(cfg: ToastConfig, target: HTMLElement): void {
  let dismissTimer: ReturnType<typeof setTimeout> | null = null;

  // Auto-dismiss 5s after a message appears.
  watch(() => {
    const msg = cfg.message();
    if (msg) {
      if (dismissTimer) clearTimeout(dismissTimer);
      dismissTimer = setTimeout(() => {
        cfg.onDismiss();
        dismissTimer = null;
      }, 5000);
    }
  });

  html`
    ${() => cfg.message()
      ? html`
          <div class="toast-banner">
            <span class="toast-text">${() => cfg.message()}</span>
            <button class="toast-close" @click="${() => { cfg.onDismiss(); if (dismissTimer) { clearTimeout(dismissTimer); dismissTimer = null; } }}">&times;</button>
          </div>
        `
      : html``}
  `(target);
}
