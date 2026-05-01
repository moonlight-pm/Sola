// Semantic slot editor for components. Each slot is a *role* the
// component plays (e.g. "Border radius") that resolves to one of several
// candidate design tokens. The user can:
//   1. Pick a different candidate for the slot (e.g. swap --radius-sm
//      for --radius-lg, or set the slot to None).
//   2. Edit the underlying token's value (changes propagate to every
//      consumer of that token, not just this component).
//
// Mechanism: each component's CSS reads from per-component CSS aliases
// (`--kit-btn-radius`, etc.), with a default mapping in :root that
// points each alias at a base token. Picking a different candidate
// updates the alias's :root value to point at the new token. None
// unsets the alias to a no-op value (`0`, `transparent`, etc., per
// the slot's kind).
//
// Slot overrides are in-memory only for now; persistence via the bus
// topic is a follow-up (would require a component_slots field in the
// Theme schema).

import { html, reactive, type TemplatePartial } from '@arrow-js/core';
import { themeState, setColor, setSpacing, setRadius, setTypography } from '../token-edit.js';
import { pickerSwatch } from '../color-picker.js';
import { fontPicker } from '../font-picker.js';

export type SlotKind = 'color' | 'spacing' | 'radius' | 'text-size' | 'font';

export interface Slot {
  /** CSS alias variable, e.g. '--kit-btn-radius'. */
  alias: string;
  /** Slot label shown in UI, e.g. 'Corner radius'. */
  label: string;
  /** Plain-language description of what this slot affects. */
  description: string;
  /** Token kind — controls the candidate pool and the editor type. */
  kind: SlotKind;
  /** Base token the slot resolves to by default, e.g. '--radius-sm'. */
  defaultToken: string;
  /** Whether the user can clear this slot (sets the alias to a no-op). */
  allowNone?: boolean;
}

export interface SlotGroup {
  id: string;
  label: string;
  description: string;
  slots: Slot[];
}

export interface ComponentSlots {
  groups: SlotGroup[];
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

function candidatesFor(kind: SlotKind): string[] {
  switch (kind) {
    case 'color': return COLOR_TOKENS;
    case 'spacing': return SPACING_TOKENS;
    case 'radius': return RADIUS_TOKENS;
    case 'text-size': return TEXT_SIZE_TOKENS;
    case 'font': return FONT_TOKENS;
  }
}

// "None" means: set the alias to a no-op value of the appropriate kind.
function noneValue(kind: SlotKind): string {
  switch (kind) {
    case 'color': return 'transparent';
    case 'spacing': return '0';
    case 'radius': return '0';
    case 'text-size': return 'inherit';
    case 'font': return 'inherit';
  }
}

// ----- In-memory slot overrides -----
//
// Map alias → currently-selected token (or 'NONE' for cleared).
// Default = whatever the slot's defaultToken is.

const overrides = reactive<{ map: Record<string, string> }>({ map: {} });

/** Returns the token currently driving an alias. 'NONE' means cleared. */
export function currentTokenFor(slot: Slot): string {
  return overrides.map[slot.alias] ?? slot.defaultToken;
}

/** Update the alias's :root rule so the new token (or none) takes effect. */
export function setSlotToken(slot: Slot, token: string | 'NONE') {
  const root = document.documentElement;
  if (token === 'NONE') {
    root.style.setProperty(slot.alias, noneValue(slot.kind));
    overrides.map = { ...overrides.map, [slot.alias]: 'NONE' };
  } else {
    root.style.setProperty(slot.alias, `var(${token})`);
    overrides.map = { ...overrides.map, [slot.alias]: token };
  }
}

/** Reset a slot to its default token. */
export function resetSlot(slot: Slot) {
  const root = document.documentElement;
  root.style.setProperty(slot.alias, `var(${slot.defaultToken})`);
  const next = { ...overrides.map };
  delete next[slot.alias];
  overrides.map = next;
}

// ----- Token-value editing helpers (drive the existing token store) -----

interface FontList { sans: string[]; mono: string[] }
function fonts(): FontList {
  return ((window as unknown as { RESTORED_STATE?: { fonts?: FontList } }).RESTORED_STATE?.fonts) ?? { sans: [], mono: [] };
}

function valueForToken(token: string, kind: SlotKind): () => string {
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

function setValueForToken(token: string, kind: SlotKind, v: string) {
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

export function renderSlots(spec: ComponentSlots): TemplatePartial {
  return html`
    <div class="kit-slot-view">
      ${spec.groups.map(group => renderGroup(group))}
    </div>
  `;
}

function renderGroup(group: SlotGroup) {
  return html`
    <section class="kit-slot-group">
      <header class="kit-slot-group-head">
        <h3 class="kit-slot-group-title">${group.label}</h3>
        <p class="kit-slot-group-desc">${group.description}</p>
      </header>
      <div class="kit-slot-list">
        ${() => group.slots.map(slot => renderSlot(slot).key(`${group.id}:${slot.alias}`))}
      </div>
    </section>
  `;
}

function renderSlot(slot: Slot) {
  const isOverridden = () => overrides.map[slot.alias] !== undefined;
  const currentToken = (): string => currentTokenFor(slot);
  return html`
    <div class="kit-slot">
      <div class="kit-slot-head">
        <div class="kit-slot-meta">
          <div class="kit-slot-label">${slot.label}</div>
          <div class="kit-slot-desc">${slot.description}</div>
        </div>
        ${() => isOverridden()
          ? html`<button class="kit-slot-reset" @click="${() => resetSlot(slot)}">reset</button>`
          : html``}
      </div>
      <div class="kit-slot-body">
        ${() => tokenPicker(slot, currentToken)}
        ${() => valueEditor(slot, currentToken)}
      </div>
    </div>
  `;
}

// ----- Per-slot widgets -----

const pickerLocal = reactive<{ openAlias: string | null }>({ openAlias: null });

document.addEventListener('click', (e) => {
  if (pickerLocal.openAlias === null) return;
  const path = (e.composedPath ? e.composedPath() : []) as EventTarget[];
  const inside = path.some((n) => {
    const el = n as HTMLElement;
    return el.classList && (
      el.classList.contains('kit-slot-token-trigger') ||
      el.classList.contains('kit-slot-token-popover')
    );
  });
  if (!inside) pickerLocal.openAlias = null;
});

function tokenPicker(slot: Slot, currentToken: () => string) {
  const isOpen = () => pickerLocal.openAlias === slot.alias;
  const candidates = candidatesFor(slot.kind);
  return html`<div class="kit-slot-token-wrap">
    <button class="kit-slot-token-trigger"
      @click="${(e: Event) => {
        e.stopPropagation();
        pickerLocal.openAlias = isOpen() ? null : slot.alias;
      }}">
      <span class="kit-slot-token-name">${currentToken}</span>
      <span class="kit-slot-token-chev">▾</span>
    </button>
    ${() => isOpen()
      ? html`<div class="kit-slot-token-popover" @click="${(e: Event) => e.stopPropagation()}">
          ${candidates.map(token => html`<button
            class="kit-slot-token-option"
            data-active="${() => currentToken() === token ? 'active' : false}"
            @click="${() => { setSlotToken(slot, token); pickerLocal.openAlias = null; }}">${token}</button>`)}
          ${slot.allowNone ? html`<button
            class="kit-slot-token-option kit-slot-token-option-none"
            data-active="${() => currentToken() === 'NONE' ? 'active' : false}"
            @click="${() => { setSlotToken(slot, 'NONE'); pickerLocal.openAlias = null; }}">None</button>` : html``}
        </div>`
      : html``}
  </div>`;
}

function valueEditor(slot: Slot, currentToken: () => string) {
  const token = currentToken();
  if (token === 'NONE') {
    return html`<span class="kit-slot-none">unset</span>`;
  }
  const value = valueForToken(token, slot.kind);

  if (slot.kind === 'color') {
    const fieldName = token.slice(2).replaceAll('-', '_');
    return html`<div class="kit-slot-value">
      ${() => pickerSwatch({
        id: `slot:${slot.alias}`,
        value,
        onChange: (v: string) => setColor(fieldName, v),
        className: 'kit-slot-swatch',
      })}
      <input class="kit-field" value="${value}"
        @input="${(e: Event) => setValueForToken(token, slot.kind, (e.target as HTMLInputElement).value)}">
    </div>`;
  }

  if (slot.kind === 'font') {
    const fieldName = token.slice(2).replaceAll('-', '_');
    const isMono = token === '--font-mono';
    return html`<div class="kit-slot-value">
      ${() => fontPicker({
        id: `slot:${slot.alias}`,
        value,
        options: () => isMono ? fonts().mono : fonts().sans,
        onChange: (v: string) => setTypography(fieldName, v),
      })}
    </div>`;
  }

  // Spacing / radius / text-size — plain text input
  return html`<div class="kit-slot-value">
    <input class="kit-field" value="${value}"
      @input="${(e: Event) => setValueForToken(token, slot.kind, (e.target as HTMLInputElement).value)}">
  </div>`;
}
