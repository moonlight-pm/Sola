import { component, html, type TemplatePartial } from '@arrow-js/core';

export interface SidebarOpts {
  title?: string | (() => string);
  body: TemplatePartial;
}

export const sidebarTokens = [
  '--bg-secondary', '--border-subtle', '--text-muted',
  '--space-xs', '--space-sm', '--space-md',
  '--text-caption',
];

export const sidebar = component((props: SidebarOpts) => {
  const titleFn = () => typeof props.title === 'function' ? (props.title as () => string)() : props.title;
  return html`<aside class="kit-sidebar">
    ${() => props.title ? html`<div class="kit-sidebar-title">${titleFn}</div>` : null}
    ${() => props.body}
  </aside>`;
});
