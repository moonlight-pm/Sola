import { html } from '@arrow-js/core';

export interface NavItemOpts {
  label: string | (() => string);
  active?: boolean | (() => boolean);
  onClick?: () => void;
}

export const navItemTokens = [
  '--text-secondary', '--text-primary',
  '--bg-tertiary', '--accent', '--accent-dim',
  '--radius-sm', '--text-body', '--space-xs', '--space-sm',
];

export function navItem(opts: NavItemOpts) {
  const activeAttr = (): string | false => {
    const a = typeof opts.active === 'function' ? opts.active() : opts.active;
    return a ? 'active' : false;
  };
  return html`<button
    class="kit-nav-item"
    data-active="${activeAttr}"
    @click=${() => opts.onClick && opts.onClick()}
  >${typeof opts.label === 'function' ? opts.label : () => opts.label}</button>`;
}
