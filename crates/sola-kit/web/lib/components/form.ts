import { component, html, type TemplatePartial } from '@arrow-js/core';

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

export const form = component((props: FormOpts) =>
  html`<div class="kit-form">
    <div class="kit-form-body">${() => props.body}</div>
    ${() => props.actions ? html`<div class="kit-form-actions">${() => props.actions}</div>` : null}
  </div>`
);

export const fieldRow = component((props: FieldRowOpts) =>
  html`<div class="kit-field-row">
    <label class="kit-field-label">${() => props.label}</label>
    <div class="${() => `kit-field-body kit-field-${props.width ?? 'normal'}`}">${() => props.body}</div>
  </div>`
);
