import { html, type TemplatePartial } from '@arrow-js/core';

export interface SidebarOpts {
  title?: string | (() => string);
  body: TemplatePartial;
}

export const sidebarTokens = [
  '--bg-secondary', '--border-subtle', '--text-muted',
  '--space-xs', '--space-sm', '--space-md',
  '--text-caption',
];

export function sidebar(opts: SidebarOpts) {
  return html`<aside class="kit-sidebar">
    ${() => opts.title ? html`<div class="kit-sidebar-title">${
      typeof opts.title === 'function' ? opts.title : () => opts.title
    }</div>` : html``}
    ${() => opts.body}
  </aside>`;
}
