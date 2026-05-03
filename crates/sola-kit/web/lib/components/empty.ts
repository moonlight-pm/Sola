import { component, html } from '@arrow-js/core';

export interface EmptyOpts {
  label: string | (() => string);
  hint?: string | (() => string);
}

export const emptyTokens = [
  '--text-muted',
  '--text-body', '--text-caption',
  '--space-md',
];

export const empty = component((props: EmptyOpts) => {
  const labelFn = () => typeof props.label === 'function' ? (props.label as () => string)() : props.label;
  const hintFn = () => typeof props.hint === 'function' ? (props.hint as () => string)() : props.hint;
  return html`<div class="kit-empty">
    <div class="kit-empty-label">${labelFn}</div>
    ${() => props.hint ? html`<div class="kit-empty-hint">${hintFn}</div>` : null}
  </div>`;
});
