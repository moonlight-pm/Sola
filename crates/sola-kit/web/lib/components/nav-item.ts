import { component, html } from '@arrow-js/core';

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

export const navItem = component((props: NavItemOpts) => {
  const labelFn = () => typeof props.label === 'function' ? (props.label as () => string)() : props.label;
  const activeAttr = (): string | false => {
    const a = typeof props.active === 'function' ? (props.active as () => boolean)() : props.active;
    return a ? 'active' : false;
  };
  return html`<button
    class="kit-nav-item"
    data-active="${activeAttr}"
    @click="${() => props.onClick?.()}"
  >${labelFn}</button>`;
});
