import { component, html } from '@arrow-js/core';

export interface IconOpts {
  name: string | (() => string);
  size?: number;
}

export const iconTokens = ['--text-secondary'];

export const icon = component((props: IconOpts) => {
  const nameFn = () => typeof props.name === 'function' ? (props.name as () => string)() : props.name;
  return html`<img
    class="kit-icon"
    src="${() => `sola-assets://icons/${nameFn()}.svg`}"
    width="${() => props.size ?? 16}"
    height="${() => props.size ?? 16}"
  >`;
});
