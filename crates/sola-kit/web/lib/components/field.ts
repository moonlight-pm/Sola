import { html } from '@arrow-js/core';

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

export function field(opts: FieldOpts) {
  const t = opts.type ?? 'text';
  const valueExpr = typeof opts.value === 'function' ? opts.value : () => opts.value as string;
  const errorExpr = (): string | false => {
    const e = typeof opts.error === 'function' ? opts.error() : opts.error;
    return e ? 'error' : false;
  };
  return html`<input
    type="${t}"
    class="kit-field"
    data-error="${errorExpr}"
    placeholder="${opts.placeholder ?? ''}"
    value="${valueExpr}"
    @input=${(e: Event) => opts.onInput && opts.onInput((e.target as HTMLInputElement).value)}
  >`;
}
