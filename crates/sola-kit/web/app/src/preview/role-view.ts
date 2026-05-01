// Semantic role editor for components. Each role is a *role* the
// component plays (e.g. "Border radius") that resolves to one of several
// candidate design tokens. The user can:
//   1. Pick a different candidate for the role (e.g. swap --radius-sm
//      for --radius-lg, or set the role to None).
//   2. Edit the underlying token's value (changes propagate to every
//      consumer of that token, not just this component).
//
// Mechanism: each component's CSS reads from per-component CSS aliases
// (`--kit-btn-radius`, etc.), with a default mapping in :root that
// points each alias at a base token. Picking a different candidate
// updates the alias's :root value to point at the new token. None
// unsets the alias to a no-op value (`0`, `transparent`, etc., per
// the role's kind).
//
// Role overrides are in-memory only for now; persistence via the bus
// topic is a follow-up (would require a component_roles field in the
// Theme schema).

import { html, reactive, type TemplatePartial } from '@arrow-js/core';
import { themeState, setColor, setSpacing, setRadius, setTypography } from '../token-edit.js';
import { pickerSwatch } from '../color-picker.js';
import { fontPicker } from '../font-picker.js';

export type RoleKind = 'color' | 'spacing' | 'radius' | 'text-size' | 'font';

export interface Role {
  /** CSS alias variable, e.g. '--kit-btn-radius'. */
  alias: string;
  /** Role label shown in UI, e.g. 'Corner radius'. */
  label: string;
  /** Plain-language description of what this role affects. */
  description: string;
  /** Token kind — controls the candidate pool and the editor type. */
  kind: RoleKind;
  /** Base token the role resolves to by default, e.g. '--radius-sm'. */
  defaultToken: string;
  /** Whether the user can clear this role (sets the alias to a no-op). */
  allowNone?: boolean;
}

export interface RoleGroup {
  id: string;
  label: string;
  description: string;
  roles: Role[];
}

export interface ComponentRoles {
  groups: RoleGroup[];
}

// ----- Token candidate pools (which tokens are choosable per kind) -----

const COLOR_TOKENS = [
  '--bg-primary', '--bg-secondary', '--bg-tertiary', '--bg-hover',
  '--border', '--border-subtle',
  '--text-primary', '--text-secondary', '--text-tertiary', '--text-muted', '--text-accent',
  '--accent', '--accent-dim',
  '--danger', '--success',
];
const SPACING_TOKENS = ['--space-xs', '--space-sm', '--space-md', '--space-lg', '--space-xl', '--space-xxl'];
const RADIUS_TOKENS = ['--radius-sm', '--radius-md', '--radius-lg'];
const TEXT_SIZE_TOKENS = ['--text-caption', '--text-body', '--text-body-lg', '--text-heading', '--text-display'];
const FONT_TOKENS = ['--font-sans', '--font-mono'];

function candidatesFor(kind: RoleKind): string[] {
  switch (kind) {
    case 'color': return COLOR_TOKENS;
    case 'spacing': return SPACING_TOKENS;
    case 'radius': return RADIUS_TOKENS;
    case 'text-size': return TEXT_SIZE_TOKENS;
    case 'font': return FONT_TOKENS;
  }
}

// "None" means: set the alias to a no-op value of the appropriate kind.
function noneValue(kind: RoleKind): string {
  switch (kind) {
    case 'color': return 'transparent';
    case 'spacing': return '0';
    case 'radius': return '0';
    case 'text-size': return 'inherit';
    case 'font': return 'inherit';
  }
}

// ----- In-memory role overrides -----
//
// Map alias → currently-selected token (or 'NONE' for cleared).
// Default = whatever the role's defaultToken is.

const overrides = reactive<{ map: Record<string, string> }>({ map: {} });

/** Returns the token currently driving an alias. 'NONE' means cleared. */
export function currentTokenFor(role: Role): string {
  return overrides.map[role.alias] ?? role.defaultToken;
}

/** Update the alias's :root rule so the new token (or none) takes effect. */
export function setRoleToken(role: Role, token: string | 'NONE') {
  const root = document.documentElement;
  if (token === 'NONE') {
    root.style.setProperty(role.alias, noneValue(role.kind));
    overrides.map = { ...overrides.map, [role.alias]: 'NONE' };
  } else {
    root.style.setProperty(role.alias, `var(${token})`);
    overrides.map = { ...overrides.map, [role.alias]: token };
  }
}

/** Reset a role to its default token. */
export function resetRole(role: Role) {
  const root = document.documentElement;
  root.style.setProperty(role.alias, `var(${role.defaultToken})`);
  const next = { ...overrides.map };
  delete next[role.alias];
  overrides.map = next;
}

// ----- Token-value editing helpers (drive the existing token store) -----

interface FontList { sans: string[]; mono: string[] }
function fonts(): FontList {
  return ((window as unknown as { RESTORED_STATE?: { fonts?: FontList } }).RESTORED_STATE?.fonts) ?? { sans: [], mono: [] };
}

function valueForToken(token: string, kind: RoleKind): () => string {
  if (token === 'NONE') return () => '—';
  const t = themeState.current as Record<string, Record<string, string>> | undefined;
  if (kind === 'color') {
    const f = token.slice(2).replaceAll('-', '_');
    return () => t?.colors?.[f] ?? '';
  }
  if (kind === 'spacing') {
    const f = token.slice('--space-'.length);
    return () => t?.spacing?.[f] ?? '';
  }
  if (kind === 'radius') {
    const f = token.slice('--radius-'.length);
    return () => t?.radius?.[f] ?? '';
  }
  if (kind === 'text-size' || kind === 'font') {
    const f = token.slice(2).replaceAll('-', '_');
    return () => t?.typography?.[f] ?? '';
  }
  return () => '';
}

function setValueForToken(token: string, kind: RoleKind, v: string) {
  if (token === 'NONE') return;
  if (kind === 'color') {
    const f = token.slice(2).replaceAll('-', '_');
    setColor(f, v);
  } else if (kind === 'spacing') {
    setSpacing(token.slice('--space-'.length), v);
  } else if (kind === 'radius') {
    setRadius(token.slice('--radius-'.length), v);
  } else if (kind === 'text-size' || kind === 'font') {
    setTypography(token.slice(2).replaceAll('-', '_'), v);
  }
}

// ----- Top-level rendering -----

export function renderRoles(spec: ComponentRoles): TemplatePartial {
  return html`
    <div class="kit-role-view">
      ${spec.groups.map(group => renderGroup(group))}
    </div>
  `;
}

function renderGroup(group: RoleGroup) {
  return html`
    <section class="kit-role-group">
      <header class="kit-role-group-head">
        <h3 class="kit-role-group-title">${group.label}</h3>
        <p class="kit-role-group-desc">${group.description}</p>
      </header>
      <div class="kit-role-list">
        ${() => group.roles.map(role => renderRole(role).key(`${group.id}:${role.alias}`))}
      </div>
    </section>
  `;
}

function renderRole(role: Role) {
  const isOverridden = () => overrides.map[role.alias] !== undefined;
  const currentToken = (): string => currentTokenFor(role);
  return html`
    <div class="kit-role">
      <div class="kit-role-head">
        <div class="kit-role-meta">
          <div class="kit-role-label">${role.label}</div>
          <div class="kit-role-desc">${role.description}</div>
        </div>
        ${() => isOverridden()
          ? html`<button class="kit-role-reset" @click="${() => resetRole(role)}">reset</button>`
          : html``}
      </div>
      <div class="kit-role-body">
        ${() => tokenPicker(role, currentToken)}
        ${() => valueEditor(role, currentToken)}
      </div>
    </div>
  `;
}

// ----- Per-role widgets -----

const pickerLocal = reactive<{ openAlias: string | null }>({ openAlias: null });

document.addEventListener('click', (e) => {
  if (pickerLocal.openAlias === null) return;
  const path = (e.composedPath ? e.composedPath() : []) as EventTarget[];
  const inside = path.some((n) => {
    const el = n as HTMLElement;
    return el.classList && (
      el.classList.contains('kit-role-token-trigger') ||
      el.classList.contains('kit-role-token-popover')
    );
  });
  if (!inside) pickerLocal.openAlias = null;
});

function tokenPicker(role: Role, currentToken: () => string) {
  const isOpen = () => pickerLocal.openAlias === role.alias;
  const candidates = candidatesFor(role.kind);
  return html`<div class="kit-role-token-wrap">
    <button class="kit-role-token-trigger"
      @click="${(e: Event) => {
        e.stopPropagation();
        pickerLocal.openAlias = isOpen() ? null : role.alias;
      }}">
      <span class="kit-role-token-name">${currentToken}</span>
      <span class="kit-role-token-chev">▾</span>
    </button>
    ${() => isOpen()
      ? html`<div class="kit-role-token-popover" @click="${(e: Event) => e.stopPropagation()}">
          ${candidates.map(token => html`<button
            class="kit-role-token-option"
            data-active="${() => currentToken() === token ? 'active' : false}"
            @click="${() => { setRoleToken(role, token); pickerLocal.openAlias = null; }}">${token}</button>`)}
          ${role.allowNone ? html`<button
            class="kit-role-token-option kit-role-token-option-none"
            data-active="${() => currentToken() === 'NONE' ? 'active' : false}"
            @click="${() => { setRoleToken(role, 'NONE'); pickerLocal.openAlias = null; }}">None</button>` : html``}
        </div>`
      : html``}
  </div>`;
}

function valueEditor(role: Role, currentToken: () => string) {
  const token = currentToken();
  if (token === 'NONE') {
    return html`<span class="kit-role-none">unset</span>`;
  }
  const value = valueForToken(token, role.kind);

  if (role.kind === 'color') {
    const fieldName = token.slice(2).replaceAll('-', '_');
    return html`<div class="kit-role-value">
      ${() => pickerSwatch({
        id: `role:${role.alias}`,
        value,
        onChange: (v: string) => setColor(fieldName, v),
        className: 'kit-role-swatch',
      })}
      <input class="kit-field" value="${value}"
        @input="${(e: Event) => setValueForToken(token, role.kind, (e.target as HTMLInputElement).value)}">
    </div>`;
  }

  if (role.kind === 'font') {
    const fieldName = token.slice(2).replaceAll('-', '_');
    const isMono = token === '--font-mono';
    return html`<div class="kit-role-value">
      ${() => fontPicker({
        id: `role:${role.alias}`,
        value,
        options: () => isMono ? fonts().mono : fonts().sans,
        onChange: (v: string) => setTypography(fieldName, v),
      })}
    </div>`;
  }

  // Spacing / radius / text-size — plain text input
  return html`<div class="kit-role-value">
    <input class="kit-field" value="${value}"
      @input="${(e: Event) => setValueForToken(token, role.kind, (e.target as HTMLInputElement).value)}">
  </div>`;
}
