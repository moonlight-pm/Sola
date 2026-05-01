import { html, type TemplatePartial } from '@arrow-js/core';
import {
  button, field, badge, icon,
  sidebar, navItem, section, row, list, form, fieldRow,
  tabs, tab, toast, empty,
} from '@sola/kit';
import { themeState, setColor } from '../token-edit';
import type { CatalogEntry } from '../sidebar';

interface ViewSpec {
  variants: () => TemplatePartial;
  notes?: string;
}

const VIEWS: Record<string, ViewSpec> = {
  button: { variants: () => html`
    <div class="kit-variants">
      ${button({ label: 'Primary', variant: 'primary' })}
      ${button({ label: 'Default' })}
      ${button({ label: 'Ghost', variant: 'ghost' })}
      ${button({ label: 'Danger', variant: 'danger' })}
      ${button({ label: '+ Add', variant: 'add' })}
    </div>
  ` },
  field: { variants: () => html`
    <div class="kit-variants kit-variants-stack">
      ${field({ value: '', placeholder: 'placeholder' })}
      ${field({ value: 'with value' })}
      ${field({ value: 'invalid', error: 'oops' })}
    </div>
  ` },
  badge: { variants: () => html`
    <div class="kit-variants">
      ${badge({ label: 'default' })}
      ${badge({ label: 'accent', variant: 'accent' })}
      ${badge({ label: 'danger', variant: 'danger' })}
      ${badge({ label: 'success', variant: 'success' })}
    </div>
  ` },
  icon: { variants: () => html`<div class="kit-variants">${icon({ name: 'lucide/palette', size: 24 })}</div>` },
  sidebar: { variants: () => html`<div class="kit-variants" style="height: 200px">${sidebar({
    title: 'Title',
    body: html`${navItem({ label: 'Item A', active: true })}${navItem({ label: 'Item B' })}`,
  })}</div>` },
  'nav-item': { variants: () => html`<div class="kit-variants kit-variants-stack">
    ${navItem({ label: 'Inactive' })}
    ${navItem({ label: 'Active', active: true })}
  </div>` },
  section: { variants: () => html`${section({ title: 'A section', description: 'A short description.', body: html`<p>Body content.</p>` })}` },
  row: { variants: () => html`<div class="kit-variants kit-variants-stack">
    ${row({ label: 'Simple row' })}
    ${row({ label: 'Row with detail', detail: '/path/to/value' })}
    ${row({ label: 'Row with actions', actions: html`${button({ label: 'Edit', variant: 'ghost' })}` })}
  </div>` },
  list: { variants: () => html`${list({ body: html`${row({ label: 'one' })}${row({ label: 'two' })}${row({ label: 'three' })}` })}` },
  form: { variants: () => html`${form({
    body: html`${fieldRow({ label: 'Email', body: field({ value: 'user@example.com' }) })}${fieldRow({ label: 'Pass', body: field({ value: '', type: 'password' }) })}`,
    actions: html`${button({ label: 'Save', variant: 'primary' })}${button({ label: 'Cancel', variant: 'ghost' })}`,
  })}` },
  tabs: { variants: () => html`<div class="kit-variants kit-variants-stack" style="width:240px">
    ${tabs({ body: html`
      ${tab({ title: 'one',   variant: 'numbered', index: 1, active: true })}
      ${tab({ title: 'two',   variant: 'numbered', index: 2 })}
      ${tab({ title: 'three', variant: 'numbered', index: 3 })}
    ` })}
  </div>` },
  toast: { variants: () => html`<div class="kit-variants kit-variants-stack" style="max-width:360px">
    ${toast({ body: html`Default toast.` })}
    ${toast({ variant: 'success', body: html`Saved successfully.` })}
    ${toast({ variant: 'danger', body: html`Operation failed.` })}
  </div>` },
  empty: { variants: () => html`${empty({ label: 'Nothing yet', hint: 'Add an item to get started.' })}` },
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
      <div class="kit-preview">${view.variants()}</div>
      <div class="kit-section-title-sm" style="margin-top: var(--space-md)">Tokens this uses · click a chip to edit</div>
      <div class="kit-chips">
        ${entry.tokens.map(varName => renderChip(varName))}
      </div>
    </div>
  `;
}

function renderChip(varName: string) {
  // Map var → struct field (e.g. "--accent-dim" → "accent_dim")
  const colorField = stripPrefix(varName, '--')?.replaceAll('-', '_');
  const isColor = colorField && themeState.current?.colors && (colorField in themeState.current.colors);
  if (isColor) {
    const valueExpr = (): string => themeState.current?.colors?.[colorField!] ?? '';
    return html`<label class="kit-chip">
      <span class="kit-chip-swatch" style="${() => `background: ${valueExpr()}`}"></span>
      <span class="kit-chip-name">${varName}</span>
      <input type="color" value="${() => normaliseToHex(valueExpr())}" @input=${(e: Event) => setColor(colorField!, (e.target as HTMLInputElement).value)}>
    </label>`;
  }
  // Non-color tokens (typography, spacing, radius) — show value as text;
  // editing routes to the token-mode views for now.
  return html`<span class="kit-chip">
    <span class="kit-chip-name">${varName}</span>
  </span>`;
}

function normaliseToHex(value: string): string {
  if (value.startsWith('#') && (value.length === 7 || value.length === 4)) return value;
  return '#000000';
}

function stripPrefix(s: string, p: string): string | null {
  return s.startsWith(p) ? s.slice(p.length) : null;
}
