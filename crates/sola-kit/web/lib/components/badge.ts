import { component, html } from '@arrow-js/core';

export type BadgeVariant = 'default' | 'accent' | 'danger' | 'success';

export interface BadgeOpts {
  label: string | (() => string);
  variant?: BadgeVariant;
}

export const badgeTokens = [
  '--bg-tertiary', '--text-secondary',
  '--accent', '--accent-dim',
  '--danger', '--success',
  '--radius-sm', '--text-caption', '--space-xs',
];

export const badge = component((props: BadgeOpts) => {
  const labelFn = () => typeof props.label === 'function' ? (props.label as () => string)() : props.label;
  return html`<span class="${() => `kit-badge kit-badge-${props.variant ?? 'default'}`}">${labelFn}</span>`;
});
