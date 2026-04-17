import { html } from '@arrow-js/core';

export interface ToastsConfig {
  downloads: () => Array<{ id: string; filename: string; progress: number }>;
}

export function createToasts(config: ToastsConfig, target: HTMLElement): void {
  html`
    <div class="toast-stack">
      ${() => config.downloads().map(d =>
        html`<div class="download-toast">
          <span class="download-filename">${() => d.filename}</span>
          <span class="download-progress">${() => Math.round(d.progress * 100)}%</span>
        </div>`
      )}
    </div>
  `(target);
}
