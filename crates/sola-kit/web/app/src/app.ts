import { component, html, reactive } from '@arrow-js/core';
import { appSidebar, type CatalogEntry } from './sidebar.js';
import { colorsView } from './preview/tokens-colors.js';
import { typographyView } from './preview/tokens-typography.js';
import { spacingView } from './preview/tokens-spacing.js';
import { componentView } from './preview/component-view.js';

declare global {
  interface Window { RESTORED_STATE?: { catalog: CatalogEntry[]; theme: unknown }; }
}

const restored = window.RESTORED_STATE ?? { catalog: [], theme: null };

const state = reactive({
  selected: 'tokens.colors',
  catalog: restored.catalog as CatalogEntry[],
});

// Each route is mounted ONCE, keyed by id. Active state propagates as a
// data-active attribute; CSS hides inactive panels with display:none.
// This is the idiomatic Arrow pattern (see arrow-js/docs/play/examples/tabs):
// no template swapping ever happens at the route boundary, which avoids
// the patch-path failure modes around `getNode(prev).after(fragment); unmount(prev)`.
const routePanel = component((props: { active: boolean; body: () => unknown }) =>
  html`<div class="kit-route-panel" data-active="${() => String(props.active)}">${() => props.body()}</div>`
);

interface Route {
  id: string;
  body: () => unknown;
}

function buildRoutes(catalog: CatalogEntry[]): Route[] {
  const r: Route[] = [
    { id: 'tokens.colors',     body: () => colorsView({ catalog }) },
    { id: 'tokens.typography', body: () => typographyView() },
    { id: 'tokens.spacing',    body: () => spacingView() },
  ];
  for (const a of catalog.filter(e => e.group === 'atom')) {
    r.push({ id: `atoms.${a.name}`, body: () => componentView({ name: a.name, catalog }) });
  }
  for (const c of catalog.filter(e => e.group === 'component')) {
    r.push({ id: `components.${c.name}`, body: () => componentView({ name: c.name, catalog }) });
  }
  return r;
}

export function mount(target: HTMLElement) {
  html`<div class="kit-shell">
    ${() => appSidebar({
      state,
      catalog: state.catalog,
      onSelect: (id: string) => { state.selected = id; },
    })}
    <main class="kit-work">
      ${() => buildRoutes(state.catalog).map(r =>
        routePanel({
          active: state.selected === r.id,
          body: r.body,
        }).key(r.id)
      )}
    </main>
  </div>`(target);
}
