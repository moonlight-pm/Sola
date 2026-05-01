import { html, type TemplatePartial } from '@arrow-js/core';
import {
  button, field, badge, icon,
  sidebar, navItem, section, row, list, form, fieldRow,
  tabs, tab, toast, empty,
} from '@sola/kit';
import {
  themeState,
  setColor, setTypography, setSpacing, setRadius,
} from '../token-edit.js';
import { pickerSwatch } from '../color-picker.js';
import { fontPicker } from '../font-picker.js';
import type { CatalogEntry } from '../sidebar.js';

interface ViewSpec {
  variants: () => TemplatePartial;
}

interface FontList { sans: string[]; mono: string[] }
function fonts(): FontList {
  return ((window as unknown as { RESTORED_STATE?: { fonts?: FontList } }).RESTORED_STATE?.fonts) ?? { sans: [], mono: [] };
}

// NB: every nested ${child(...)} interpolation MUST be wrapped in
// ${() => child(...)} — many of these variants share a raw-strings
// signature (e.g. `<div class="kit-variants">${}${}${}${}</div>`) and
// Arrow's chunk cache would otherwise reuse the previous view's chunk
// across navigation and never re-mount the children, leaving stale DOM.
const VIEWS: Record<string, ViewSpec> = {
  button: { variants: () => html`
    <div class="kit-variants">
      ${() => button({ label: 'Primary', variant: 'primary' })}
      ${() => button({ label: 'Default' })}
      ${() => button({ label: 'Ghost', variant: 'ghost' })}
      ${() => button({ label: 'Danger', variant: 'danger' })}
    </div>
  ` },
  field: { variants: () => html`
    <div class="kit-variants kit-variants-stack">
      ${() => field({ value: '', placeholder: 'placeholder' })}
      ${() => field({ value: 'with value' })}
      ${() => field({ value: 'invalid', error: 'oops' })}
    </div>
  ` },
  badge: { variants: () => html`
    <div class="kit-variants">
      ${() => badge({ label: 'default' })}
      ${() => badge({ label: 'accent', variant: 'accent' })}
      ${() => badge({ label: 'danger', variant: 'danger' })}
      ${() => badge({ label: 'success', variant: 'success' })}
    </div>
  ` },
  icon: { variants: () => html`<div class="kit-variants">${() => icon({ name: 'lucide/palette', size: 24 })}</div>` },
  sidebar: { variants: () => html`<div class="kit-variants" style="height: 200px">${() => sidebar({
    title: 'Title',
    body: html`${() => navItem({ label: 'Item A', active: true })}${() => navItem({ label: 'Item B' })}`,
  })}</div>` },
  'nav-item': { variants: () => html`<div class="kit-variants kit-variants-stack">
    ${() => navItem({ label: 'Inactive' })}
    ${() => navItem({ label: 'Active', active: true })}
  </div>` },
  section: { variants: () => html`${() => section({ title: 'A section', description: 'A short description.', body: html`<p>Body content.</p>` })}` },
  row: { variants: () => html`<div class="kit-variants kit-variants-stack">
    ${() => row({ label: 'Simple row' })}
    ${() => row({ label: 'Row with detail', detail: '/path/to/value' })}
    ${() => row({ label: 'Row with actions', actions: html`${() => button({ label: 'Edit', variant: 'ghost' })}` })}
  </div>` },
  list: { variants: () => html`${() => list({ body: html`${() => row({ label: 'one' })}${() => row({ label: 'two' })}${() => row({ label: 'three' })}` })}` },
  form: { variants: () => html`${() => form({
    body: html`${() => fieldRow({ label: 'Email', body: field({ value: 'user@example.com' }) })}${() => fieldRow({ label: 'Pass', body: field({ value: '', type: 'password' }) })}`,
    actions: html`${() => button({ label: 'Save', variant: 'primary' })}${() => button({ label: 'Cancel', variant: 'ghost' })}`,
  })}` },
  tabs: { variants: () => html`<div class="kit-variants kit-variants-stack" style="width:240px">
    ${() => tabs({ body: html`
      ${() => tab({ title: 'one',   variant: 'numbered', index: 1, active: true })}
      ${() => tab({ title: 'two',   variant: 'numbered', index: 2 })}
      ${() => tab({ title: 'three', variant: 'numbered', index: 3 })}
    ` })}
  </div>` },
  toast: { variants: () => html`<div class="kit-variants kit-variants-stack" style="max-width:360px">
    ${() => toast({ body: html`Default toast.` })}
    ${() => toast({ variant: 'success', body: html`Saved successfully.` })}
    ${() => toast({ variant: 'danger', body: html`Operation failed.` })}
  </div>` },
  empty: { variants: () => html`${() => empty({ label: 'Nothing yet', hint: 'Add an item to get started.' })}` },
};

export function renderComponent(name: string, catalog: CatalogEntry[]) {
  const view = VIEWS[name];
  const entry = catalog.find(c => c.name === name);
  if (!view || !entry) {
    return html`<div class="kit-placeholder">No preview for ${name}</div>`;
  }
  return html`
    <div class="kit-component-view">
      <div class="kit-section-title-sm">Variants</div>
      <div class="kit-preview">${() => view.variants()}</div>
      <div class="kit-section-title-sm" style="margin-top: var(--space-md)">Tokens this uses</div>
      <div class="kit-chips">
        ${() => entry.tokens.map(varName => renderChip(varName).key(varName))}
      </div>
    </div>
  `;
}

// ----- chip rendering -----

type ChipKind =
  | { kind: 'color'; field: string }
  | { kind: 'font'; field: string; isMono: boolean }
  | { kind: 'size'; group: 'typography' | 'spacing' | 'radius'; field: string };

function classify(varName: string): ChipKind {
  // var name → struct field. e.g. --bg-primary → bg_primary, --text-secondary → text_secondary.
  const fieldName = varName.slice(2).replaceAll('-', '_');

  // Color check FIRST — `--text-primary`, `--text-secondary`, etc. live in
  // colors despite the --text- prefix, while `--text-body`/`--text-heading`
  // etc. live in typography. The struct shape is authoritative.
  const colors = themeState.current?.colors as Record<string, string> | undefined;
  if (colors && fieldName in colors) {
    return { kind: 'color', field: fieldName };
  }

  if (varName.startsWith('--font-')) {
    return { kind: 'font', field: fieldName, isMono: varName === '--font-mono' };
  }

  const typo = themeState.current?.typography as Record<string, string> | undefined;
  if (typo && fieldName in typo) {
    return { kind: 'size', group: 'typography', field: fieldName };
  }

  if (varName.startsWith('--space-')) {
    return { kind: 'size', group: 'spacing', field: varName.slice('--space-'.length) };
  }
  if (varName.startsWith('--radius-')) {
    return { kind: 'size', group: 'radius', field: varName.slice('--radius-'.length) };
  }

  // Fallback: assume color (the existing colors object will return '' on miss).
  return { kind: 'color', field: fieldName };
}

function renderChip(varName: string) {
  const k = classify(varName);
  if (k.kind === 'color') return colorChip(varName, k.field);
  if (k.kind === 'font')  return fontChip(varName, k.field, k.isMono);
  return sizeChip(varName, k.group, k.field);
}

function colorChip(varName: string, field: string) {
  const value = (): string => themeState.current?.colors?.[field] ?? '';
  return html`<span class="kit-chip kit-chip-color">
    ${() => pickerSwatch({
      id: `chip:${varName}`,
      value,
      onChange: (v: string) => setColor(field, v),
      className: 'kit-chip-swatch',
    })}
    <span class="kit-chip-name">${varName}</span>
  </span>`;
}

function fontChip(varName: string, field: string, isMono: boolean) {
  const value = (): string => themeState.current?.typography?.[field] ?? '';
  return html`<span class="kit-chip kit-chip-font">
    <span class="kit-chip-name">${varName}</span>
    ${() => fontPicker({
      id: `chip:${varName}`,
      value,
      options: () => isMono ? fonts().mono : fonts().sans,
      onChange: (v: string) => setTypography(field, v),
      compact: true,
    })}
  </span>`;
}

function sizeChip(varName: string, group: 'typography' | 'spacing' | 'radius', field: string) {
  const value = (): string => {
    const t = themeState.current as Record<string, Record<string, string>> | undefined;
    return t?.[group]?.[field] ?? '';
  };
  const onChange = (v: string) => {
    if (group === 'typography') setTypography(field, v);
    else if (group === 'spacing') setSpacing(field, v);
    else setRadius(field, v);
  };
  return html`<span class="kit-chip kit-chip-size">
    <span class="kit-chip-name">${varName}</span>
    <input class="kit-chip-input" value="${value}"
      @input="${(e: Event) => onChange((e.target as HTMLInputElement).value)}">
  </span>`;
}
