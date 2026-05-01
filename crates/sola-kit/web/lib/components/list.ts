import { html, type TemplatePartial } from '@arrow-js/core';

export interface ListOpts {
  body: TemplatePartial;
}

export const listTokens = ['--space-xs'];

export function list(opts: ListOpts) {
  return html`<div class="kit-list">${() => opts.body}</div>`;
}
