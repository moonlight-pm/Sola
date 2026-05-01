import { html } from '@arrow-js/core';

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

export function badge(opts: BadgeOpts) {
  const variant = opts.variant ?? 'default';
  return html`<span class="kit-badge kit-badge-${variant}">${
    typeof opts.label === 'function' ? opts.label : () => opts.label
  }</span>`;
}
