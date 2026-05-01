import { html, type TemplatePartial } from '@arrow-js/core';

export interface RowOpts {
  label: string | (() => string);
  detail?: string | (() => string);
  actions?: TemplatePartial;
  leading?: TemplatePartial;
}

export const rowTokens = [
  '--bg-secondary', '--text-primary', '--text-tertiary',
  '--radius-md', '--text-body', '--text-caption', '--font-mono',
  '--space-sm', '--space-md',
];

export function row(opts: RowOpts) {
  const label = typeof opts.label === 'function' ? opts.label : () => opts.label;
  const detail = opts.detail
    ? (typeof opts.detail === 'function' ? opts.detail : () => opts.detail as string)
    : null;
  return html`<div class="kit-row">
    ${opts.leading ? html`<div class="kit-row-leading">${() => opts.leading}</div>` : html``}
    <div class="kit-row-info">
      <div class="kit-row-label">${label}</div>
      ${detail ? html`<div class="kit-row-detail">${detail}</div>` : html``}
    </div>
    ${opts.actions ? html`<div class="kit-row-actions">${() => opts.actions}</div>` : html``}
  </div>`;
}
