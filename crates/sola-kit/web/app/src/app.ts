import { html, reactive } from '@arrow-js/core';
import { renderSidebar, type CatalogEntry } from './sidebar.js';
import { renderColors } from './preview/tokens-colors.js';
import { renderTypography } from './preview/tokens-typography.js';
import { renderSpacing } from './preview/tokens-spacing.js';
import { renderComponent } from './preview/component-view.js';

declare global {
  interface Window { RESTORED_STATE?: { catalog: CatalogEntry[]; theme: unknown }; }
}

const restored = window.RESTORED_STATE ?? { catalog: [], theme: null };

const state = reactive({
  selected: 'tokens.colors',
  catalog: restored.catalog as CatalogEntry[],
});

export function mount(target: HTMLElement) {
  html`
    <div class="kit-shell">
      ${() => renderSidebar(
        state,
        state.catalog,
        (id: string) => { state.selected = id; },
      )}
      <main class="kit-work">
        ${() => routeWork(state.selected, state.catalog)}
      </main>
    </div>
  `(target);
}

// Route → wrapper template. Each route gets its OWN html`` literal source —
// the literal `data-route="atoms"` text differs from `data-route="components"`
// etc., giving each wrapper a different rawStrings array (and therefore a
// different chunk proto). Arrow's patch path then full-remounts on every
// navigation between groups instead of trying to syncTemplateToChunk in
// place. We further force remount WITHIN groups by routing every atom
// through atomsRoute (a single proto, but the inner `${() => renderComponent}`
// closure runs fresh on every state.selected change because the closure
// itself reads state.selected — see below).
//
// The actual cause of the intermittent stale-content bug: when two atoms
// share the renderComponent template's proto and chunk patching reuses
// the chunk in place, the inner closures *should* re-fire via
// writeExpressions → observer, but the timing of that propagation depends
// on Arrow's queue scheduling. By giving group transitions a fresh
// proto and routing intra-group navigation through a closure that
// explicitly reads state.selected, we make every navigation
// deterministically remount the body.

function routeWork(selected: string, catalog: CatalogEntry[]) {
  if (selected === 'tokens.colors')     return tokensColorsRoute(catalog);
  if (selected === 'tokens.typography') return tokensTypographyRoute();
  if (selected === 'tokens.spacing')    return tokensSpacingRoute();
  if (selected.startsWith('atoms.'))     return atomsRoute(selected, catalog);
  if (selected.startsWith('components.')) return componentsRoute(selected, catalog);
  return html`<div class="kit-placeholder">${selected}</div>`;
}

function tokensColorsRoute(catalog: CatalogEntry[]) {
  return html`<div data-route="tokens-colors" class="kit-route">${() => renderColors(catalog)}</div>`;
}
function tokensTypographyRoute() {
  return html`<div data-route="tokens-typography" class="kit-route">${() => renderTypography()}</div>`;
}
function tokensSpacingRoute() {
  return html`<div data-route="tokens-spacing" class="kit-route">${() => renderSpacing()}</div>`;
}
function atomsRoute(selected: string, catalog: CatalogEntry[]) {
  // Read state.selected inside the inner closure so the closure's tracked
  // deps include state.selected — forces a re-evaluation on every nav
  // change, not just the first mount.
  return html`<div data-route="atoms" class="kit-route">${() => {
    void state.selected;
    return renderComponent(selected.slice('atoms.'.length), catalog);
  }}</div>`;
}
function componentsRoute(selected: string, catalog: CatalogEntry[]) {
  return html`<div data-route="components" class="kit-route">${() => {
    void state.selected;
    return renderComponent(selected.slice('components.'.length), catalog);
  }}</div>`;
}
