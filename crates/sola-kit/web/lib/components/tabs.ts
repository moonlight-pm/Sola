import { component, html, type TemplatePartial } from '@arrow-js/core';

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

export const tabs = component((props: TabsOpts) =>
  html`<div class="${() => `kit-tabs kit-tabs-${props.orientation ?? 'vertical'}`}">${() => props.body}</div>`
);

export const tab = component((props: TabOpts) => {
  const titleFn = () => typeof props.title === 'function' ? (props.title as () => string)() : props.title;
  const activeAttr = (): string | false => {
    const a = typeof props.active === 'function' ? (props.active as () => boolean)() : props.active;
    return a ? 'active' : false;
  };

  // Variant shortcuts pre-fill leading / trailing. Each is computed lazily so
  // a parent can switch variant/index/faviconUrl reactively.
  const leadingFn = (): TemplatePartial | undefined => {
    if (props.leading) return props.leading;
    if (props.variant === 'numbered' && props.index !== undefined) {
      const idxFn = () => typeof props.index === 'function' ? (props.index as () => number)() : props.index as number;
      return html`<span class="kit-tab-num">${idxFn}</span>`;
    }
    if (props.variant === 'favicon' && props.faviconUrl !== undefined) {
      const urlFn = () => typeof props.faviconUrl === 'function' ? (props.faviconUrl as () => string)() : props.faviconUrl as string;
      return html`<img class="kit-tab-favicon" src="${urlFn}" width="14" height="14">`;
    }
    return undefined;
  };

  return html`<div
    class="kit-tab"
    data-active="${activeAttr}"
    @click="${() => props.onClick?.()}"
  >
    ${() => {
      const l = leadingFn();
      return l ? html`<span class="kit-tab-leading">${() => l}</span>` : null;
    }}
    <span class="kit-tab-title">${titleFn}</span>
    ${() => props.trailing ? html`<span class="kit-tab-trailing">${() => props.trailing}</span>` : null}
    ${() => props.onClose ? html`<button
      class="kit-tab-close"
      @click="${(e: Event) => { e.stopPropagation(); props.onClose?.(); }}"
    >×</button>` : null}
  </div>`;
});
