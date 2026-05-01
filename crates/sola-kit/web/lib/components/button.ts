import { html } from '@arrow-js/core';

export type ButtonVariant = 'primary' | 'default' | 'ghost' | 'danger' | 'add';

export interface ButtonOpts {
  label: string | (() => string);
  variant?: ButtonVariant;
  disabled?: boolean | (() => boolean);
  onClick?: () => void;
}

export const buttonTokens = [
  '--accent', '--accent-dim',
  '--bg-tertiary', '--text-secondary', '--text-primary',
  '--danger', '--border-subtle',
  '--radius-sm', '--text-body', '--space-sm', '--space-md',
];

export function button(opts: ButtonOpts) {
  const variant = opts.variant ?? 'default';
  const disabledAttr = (): string | false => {
    const d = typeof opts.disabled === 'function' ? opts.disabled() : opts.disabled;
    return d ? 'disabled' : false;
  };
  return html`<button
    class="kit-btn kit-btn-${variant}"
    disabled="${disabledAttr}"
    @click="${() => opts.onClick && opts.onClick()}"
  >${typeof opts.label === 'function' ? opts.label : () => opts.label}</button>`;
}
