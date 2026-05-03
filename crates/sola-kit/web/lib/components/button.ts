import { component, html } from '@arrow-js/core';

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

export const button = component((props: ButtonOpts) => {
  const labelFn = () => typeof props.label === 'function' ? (props.label as () => string)() : props.label;
  const disabledAttr = (): string | false => {
    const d = typeof props.disabled === 'function' ? (props.disabled as () => boolean)() : props.disabled;
    return d ? 'disabled' : false;
  };
  return html`<button
    class="${() => `kit-btn kit-btn-${props.variant ?? 'default'}`}"
    disabled="${disabledAttr}"
    @click="${() => props.onClick?.()}"
  >${labelFn}</button>`;
});
