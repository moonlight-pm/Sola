import { html, type TemplatePartial } from '@arrow-js/core';

export interface ToastOpts {
  body: TemplatePartial;
  variant?: 'default' | 'success' | 'danger';
}

export const toastTokens = [
  '--bg-secondary', '--border-subtle',
  '--accent', '--success', '--danger',
  '--radius-md', '--text-body',
  '--space-sm', '--space-md',
];

export function toast(opts: ToastOpts) {
  const v = opts.variant ?? 'default';
  return html`<div class="kit-toast kit-toast-${v}">${() => opts.body}</div>`;
}
