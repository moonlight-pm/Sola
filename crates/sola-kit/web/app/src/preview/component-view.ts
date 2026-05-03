import { component, html } from '@arrow-js/core';
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
import { rolesEditor, type ComponentRoles } from './role-view.js';
import { ROLE_DEFS } from './role-defs.js';
import type { CatalogEntry } from '../sidebar.js';

interface FontList { sans: string[]; mono: string[] }
function fonts(): FontList {
  return ((window as unknown as { RESTORED_STATE?: { fonts?: FontList } }).RESTORED_STATE?.fonts) ?? { sans: [], mono: [] };
}

// Each variant set is its own component so swapping between atoms goes
// through Arrow's component-swap path (factory-identity based) rather
// than the template-swap path that's bitten us repeatedly.

const buttonVariants = component(() => html`<div class="kit-variants">
  ${() => button({ label: 'Primary', variant: 'primary' })}
  ${() => button({ label: 'Default' })}
  ${() => button({ label: 'Ghost', variant: 'ghost' })}
  ${() => button({ label: 'Danger', variant: 'danger' })}
</div>`);

const fieldVariants = component(() => html`<div class="kit-variants kit-variants-stack">
  ${() => field({ value: '', placeholder: 'placeholder' })}
  ${() => field({ value: 'with value' })}
  ${() => field({ value: 'invalid', error: 'oops' })}
</div>`);

const badgeVariants = component(() => html`<div class="kit-variants">
  ${() => badge({ label: 'default' })}
  ${() => badge({ label: 'accent', variant: 'accent' })}
  ${() => badge({ label: 'danger', variant: 'danger' })}
  ${() => badge({ label: 'success', variant: 'success' })}
</div>`);

const iconVariants = component(() =>
  html`<div class="kit-variants">${() => icon({ name: 'lucide/palette', size: 24 })}</div>`
);

const sidebarVariants = component(() => html`<div class="kit-variants" style="height: 200px">${() => sidebar({
  title: 'Title',
  body: html`${() => navItem({ label: 'Item A', active: true })}${() => navItem({ label: 'Item B' })}`,
})}</div>`);

const navItemVariants = component(() => html`<div class="kit-variants kit-variants-stack">
  ${() => navItem({ label: 'Inactive' })}
  ${() => navItem({ label: 'Active', active: true })}
</div>`);

const sectionVariants = component(() =>
  html`${() => section({ title: 'A section', description: 'A short description.', body: html`<p>Body content.</p>` })}`
);

const rowVariants = component(() => html`<div class="kit-variants kit-variants-stack">
  ${() => row({ label: 'Simple row' })}
  ${() => row({ label: 'Row with detail', detail: '/path/to/value' })}
  ${() => row({ label: 'Row with actions', actions: html`${() => button({ label: 'Edit', variant: 'ghost' })}` })}
</div>`);

const listVariants = component(() =>
  html`${() => list({ body: html`${() => row({ label: 'one' })}${() => row({ label: 'two' })}${() => row({ label: 'three' })}` })}`
);

const formVariants = component(() => html`${() => form({
  body: html`${() => fieldRow({ label: 'Email', body: field({ value: 'user@example.com' }) })}${() => fieldRow({ label: 'Pass', body: field({ value: '', type: 'password' }) })}`,
  actions: html`${() => button({ label: 'Save', variant: 'primary' })}${() => button({ label: 'Cancel', variant: 'ghost' })}`,
})}`);

const tabsVariants = component(() => html`<div class="kit-variants kit-variants-stack" style="width:240px">
  ${() => tabs({ body: html`
    ${() => tab({ title: 'one',   variant: 'numbered', index: 1, active: true })}
    ${() => tab({ title: 'two',   variant: 'numbered', index: 2 })}
    ${() => tab({ title: 'three', variant: 'numbered', index: 3 })}
  ` })}
</div>`);

const toastVariants = component(() => html`<div class="kit-variants kit-variants-stack" style="max-width:360px">
  ${() => toast({ body: html`Default toast.` })}
  ${() => toast({ variant: 'success', body: html`Saved successfully.` })}
  ${() => toast({ variant: 'danger', body: html`Operation failed.` })}
</div>`);

const emptyVariants = component(() =>
  html`${() => empty({ label: 'Nothing yet', hint: 'Add an item to get started.' })}`
);

const VARIANTS: Record<string, () => ReturnType<typeof buttonVariants>> = {
  button:     buttonVariants,
  field:      fieldVariants,
  badge:      badgeVariants,
  icon:       iconVariants,
  sidebar:    sidebarVariants,
  'nav-item': navItemVariants,
  section:    sectionVariants,
  row:        rowVariants,
  list:       listVariants,
  form:       formVariants,
  tabs:       tabsVariants,
  toast:      toastVariants,
  empty:      emptyVariants,
};

interface ComponentViewProps {
  name: string;
  catalog: CatalogEntry[];
}

// Two route components — one for "has role definitions" (Variants + Roles
// editor), one for "no role definitions yet" (Variants + raw token chips).
// We dispatch between them at the componentView layer so the swap
// happens through Arrow's component-swap path (factory-identity based)
// rather than a template-swap inside one outer template (raw-strings
// proto based, the failure mode we kept hitting).

const withRolesLayout = component((props: { name: string; spec: ComponentRoles }) =>
  html`<div class="kit-component-view">
    <div class="kit-section-title-sm">Variants</div>
    <div class="kit-preview">${() => VARIANTS[props.name]?.() ?? null}</div>
    <div class="kit-section-title-sm" style="margin-top: var(--space-md)">Roles</div>
    ${() => rolesEditor(props.spec)}
  </div>`
);

const withChipsLayout = component((props: { name: string; entry: CatalogEntry }) =>
  html`<div class="kit-component-view">
    <div class="kit-section-title-sm">Variants</div>
    <div class="kit-preview">${() => VARIANTS[props.name]?.() ?? null}</div>
    <div class="kit-section-title-sm" style="margin-top: var(--space-md)">Token references</div>
    <p class="kit-section-hint">Role definitions for this component aren't authored yet — these are the raw token vars its CSS reads.</p>
    <div class="kit-chips">
      ${() => props.entry.tokens.map(varName => renderChip(varName).key(varName))}
    </div>
  </div>`
);

const placeholderLayout = component((props: { name: string }) =>
  html`<div class="kit-component-view"><div class="kit-placeholder">No preview for ${() => props.name}</div></div>`
);

export const componentView = component((props: ComponentViewProps) =>
  html`${() => {
    const variant = VARIANTS[props.name];
    const entry = props.catalog.find(c => c.name === props.name);
    if (!variant || !entry) return placeholderLayout({ name: props.name });
    const spec = ROLE_DEFS[props.name];
    return spec
      ? withRolesLayout({ name: props.name, spec })
      : withChipsLayout({ name: props.name, entry });
  }}`
);

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
  if (k.kind === 'color') return colorChip({ varName, field: k.field });
  if (k.kind === 'font')  return fontChip({ varName, field: k.field, isMono: k.isMono });
  return sizeChip({ varName, group: k.group, field: k.field });
}

const colorChip = component((props: { varName: string; field: string }) => {
  const value = (): string => themeState.current?.colors?.[props.field] ?? '';
  return html`<span class="kit-chip kit-chip-color">
    ${() => pickerSwatch({
      id: `chip:${props.varName}`,
      value,
      onChange: (v: string) => setColor(props.field, v),
      className: 'kit-chip-swatch',
    })}
    <span class="kit-chip-name">${() => props.varName}</span>
  </span>`;
});

const fontChip = component((props: { varName: string; field: string; isMono: boolean }) => {
  const value = (): string => themeState.current?.typography?.[props.field] ?? '';
  return html`<span class="kit-chip kit-chip-font">
    <span class="kit-chip-name">${() => props.varName}</span>
    ${() => fontPicker({
      id: `chip:${props.varName}`,
      value,
      options: () => props.isMono ? fonts().mono : fonts().sans,
      onChange: (v: string) => setTypography(props.field, v),
      compact: true,
    })}
  </span>`;
});

const sizeChip = component((props: { varName: string; group: 'typography' | 'spacing' | 'radius'; field: string }) => {
  const value = (): string => {
    const t = themeState.current as Record<string, Record<string, string>> | undefined;
    return t?.[props.group]?.[props.field] ?? '';
  };
  const onChange = (v: string) => {
    if (props.group === 'typography') setTypography(props.field, v);
    else if (props.group === 'spacing') setSpacing(props.field, v);
    else setRadius(props.field, v);
  };
  return html`<span class="kit-chip kit-chip-size">
    <span class="kit-chip-name">${() => props.varName}</span>
    <input class="kit-chip-input" value="${value}"
      @input="${(e: Event) => onChange((e.target as HTMLInputElement).value)}">
  </span>`;
});
