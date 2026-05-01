import { html } from '@arrow-js/core';

export interface EmptyOpts {
  label: string | (() => string);
  hint?: string | (() => string);
}

export const emptyTokens = [
  '--text-muted',
  '--text-body', '--text-caption',
  '--space-md',
];

export function empty(opts: EmptyOpts) {
  const label = typeof opts.label === 'function' ? opts.label : () => opts.label;
  const hint = opts.hint
    ? (typeof opts.hint === 'function' ? opts.hint : () => opts.hint as string)
    : null;
  return html`<div class="kit-empty">
    <div class="kit-empty-label">${label}</div>
    ${hint ? html`<div class="kit-empty-hint">${hint}</div>` : html``}
  </div>`;
}
