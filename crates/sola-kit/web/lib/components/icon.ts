import { html } from '@arrow-js/core';

export interface IconOpts {
  name: string | (() => string);
  size?: number;
}

export const iconTokens = ['--text-secondary'];

export function icon(opts: IconOpts) {
  const name = typeof opts.name === 'function' ? opts.name : () => opts.name as string;
  const size = opts.size ?? 16;
  return html`<img
    class="kit-icon"
    src="${() => `sola-assets://icons/${name()}.svg`}"
    width="${size}"
    height="${size}"
  >`;
}
