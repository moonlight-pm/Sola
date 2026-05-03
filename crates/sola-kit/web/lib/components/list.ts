import { component, html, type TemplatePartial } from '@arrow-js/core';

export interface ListOpts {
  body: TemplatePartial;
}

export const listTokens = ['--space-xs'];

export const list = component((props: ListOpts) =>
  html`<div class="kit-list">${() => props.body}</div>`
);
