import { component, html, type TemplatePartial } from '@arrow-js/core';

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

export const toast = component((props: ToastOpts) =>
  html`<div class="${() => `kit-toast kit-toast-${props.variant ?? 'default'}`}">${() => props.body}</div>`
);
