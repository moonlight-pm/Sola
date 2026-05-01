import { html, type TemplatePartial } from '@arrow-js/core';

export interface FormOpts {
  body: TemplatePartial;
  actions?: TemplatePartial;
}

export interface FieldRowOpts {
  label: string;
  body: TemplatePartial;
  width?: 'narrow' | 'normal';
}

export const formTokens = [
  '--bg-secondary', '--text-secondary',
  '--radius-md', '--text-body',
  '--space-sm', '--space-md',
];

export function form(opts: FormOpts) {
  return html`<div class="kit-form">
    <div class="kit-form-body">${() => opts.body}</div>
    ${() => opts.actions ? html`<div class="kit-form-actions">${() => opts.actions}</div>` : html``}
  </div>`;
}

export function fieldRow(opts: FieldRowOpts) {
  return html`<div class="kit-field-row">
    <label class="kit-field-label">${opts.label}</label>
    <div class="${`kit-field-body kit-field-${opts.width ?? 'normal'}`}">${() => opts.body}</div>
  </div>`;
}
