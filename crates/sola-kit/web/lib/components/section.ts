import { html, type TemplatePartial } from '@arrow-js/core';

export interface SectionOpts {
  title: string | (() => string);
  description?: string | (() => string);
  body: TemplatePartial;
}

export const sectionTokens = [
  '--text-primary', '--text-tertiary',
  '--text-heading', '--text-body',
  '--space-xs', '--space-md', '--space-lg',
];

export function section(opts: SectionOpts) {
  const title = typeof opts.title === 'function' ? opts.title : () => opts.title;
  const desc = opts.description
    ? (typeof opts.description === 'function' ? opts.description : () => opts.description as string)
    : null;
  return html`<section class="kit-section">
    <h2 class="kit-section-title">${title}</h2>
    ${() => desc ? html`<p class="kit-section-desc">${desc}</p>` : html``}
    <div class="kit-section-body">${() => opts.body}</div>
  </section>`;
}
