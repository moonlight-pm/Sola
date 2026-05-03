import { component, html } from '@arrow-js/core';

export interface FieldOpts {
  value: string | (() => string);
  onInput?: (v: string) => void;
  placeholder?: string;
  error?: string | (() => string | undefined);
  type?: 'text' | 'password' | 'email' | 'number';
}

export const fieldTokens = [
  '--bg-primary', '--border-subtle', '--accent',
  '--text-primary', '--danger',
  '--radius-sm', '--text-body', '--font-mono', '--space-xs', '--space-sm',
];

export const field = component((props: FieldOpts) => {
  const valueFn = () => typeof props.value === 'function' ? (props.value as () => string)() : props.value;
  const errorAttr = (): string | false => {
    const e = typeof props.error === 'function' ? (props.error as () => string | undefined)() : props.error;
    return e ? 'error' : false;
  };
  return html`<input
    type="${() => props.type ?? 'text'}"
    class="kit-field"
    data-error="${errorAttr}"
    placeholder="${() => props.placeholder ?? ''}"
    value="${valueFn}"
    @input="${(e: Event) => props.onInput?.((e.target as HTMLInputElement).value)}"
  >`;
});
