import { component, html, type TemplatePartial } from '@arrow-js/core';

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

export const row = component((props: RowOpts) => {
  const labelFn = () => typeof props.label === 'function' ? (props.label as () => string)() : props.label;
  const detailFn = () => typeof props.detail === 'function' ? (props.detail as () => string)() : props.detail;
  return html`<div class="kit-row">
    ${() => props.leading ? html`<div class="kit-row-leading">${() => props.leading}</div>` : null}
    <div class="kit-row-info">
      <div class="kit-row-label">${labelFn}</div>
      ${() => props.detail ? html`<div class="kit-row-detail">${detailFn}</div>` : null}
    </div>
    ${() => props.actions ? html`<div class="kit-row-actions">${() => props.actions}</div>` : null}
  </div>`;
});
