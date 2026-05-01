import { html, type TemplatePartial } from '@arrow-js/core';

export interface TabsOpts {
  body: TemplatePartial;       // a list of tab(...) calls
  orientation?: 'vertical' | 'horizontal';
}

export type TabVariant = 'numbered' | 'favicon';

export interface TabOpts {
  title: string | (() => string);
  active?: boolean | (() => boolean);
  onClick?: () => void;
  onClose?: () => void;
  leading?: TemplatePartial;       // numbered: "1", favicon: <img>
  trailing?: TemplatePartial;      // browser: reload
  variant?: TabVariant;            // shorthand for filling slots
  index?: number | (() => number); // used when variant === 'numbered'
  faviconUrl?: string | (() => string); // used when variant === 'favicon'
}

export const tabsTokens = [
  '--bg-secondary', '--bg-tertiary', '--accent-dim', '--accent',
  '--text-secondary', '--text-primary', '--border-subtle',
  '--radius-sm', '--text-body', '--text-caption', '--font-mono',
  '--space-xs', '--space-sm',
];

export function tabs(opts: TabsOpts) {
  const o = opts.orientation ?? 'vertical';
  return html`<div class="${`kit-tabs kit-tabs-${o}`}">${() => opts.body}</div>`;
}

export function tab(opts: TabOpts) {
  const activeAttr = (): string | false => {
    const a = typeof opts.active === 'function' ? opts.active() : opts.active;
    return a ? 'active' : false;
  };

  // Variant shortcuts pre-fill leading / trailing.
  let leading = opts.leading;
  let trailing = opts.trailing;
  if (opts.variant === 'numbered' && !leading && opts.index !== undefined) {
    const idx = typeof opts.index === 'function' ? opts.index : () => opts.index as number;
    leading = html`<span class="kit-tab-num">${idx}</span>`;
  }
  if (opts.variant === 'favicon' && !leading && opts.faviconUrl !== undefined) {
    const url = typeof opts.faviconUrl === 'function' ? opts.faviconUrl : () => opts.faviconUrl as string;
    leading = html`<img class="kit-tab-favicon" src="${url}" width="14" height="14">`;
  }

  const title = typeof opts.title === 'function' ? opts.title : () => opts.title;
  return html`<div
    class="kit-tab"
    data-active="${activeAttr}"
    @click="${() => opts.onClick && opts.onClick()}"
  >
    ${leading ? html`<span class="kit-tab-leading">${() => leading}</span>` : html``}
    <span class="kit-tab-title">${title}</span>
    ${trailing ? html`<span class="kit-tab-trailing">${() => trailing}</span>` : html``}
    ${opts.onClose ? html`<button
      class="kit-tab-close"
      @click="${(e: Event) => { e.stopPropagation(); opts.onClose && opts.onClose(); }}"
    >×</button>` : html``}
  </div>`;
}
