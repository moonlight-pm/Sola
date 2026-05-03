import { component, html, type TemplatePartial } from '@arrow-js/core';

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

export const section = component((props: SectionOpts) => {
  const titleFn = () => typeof props.title === 'function' ? (props.title as () => string)() : props.title;
  const descFn = () => typeof props.description === 'function' ? (props.description as () => string)() : props.description;
  return html`<section class="kit-section">
    <h2 class="kit-section-title">${titleFn}</h2>
    ${() => props.description ? html`<p class="kit-section-desc">${descFn}</p>` : null}
    <div class="kit-section-body">${() => props.body}</div>
  </section>`;
});
