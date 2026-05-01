import { html, reactive } from '@arrow-js/core';
import { renderSidebar, type CatalogEntry } from './sidebar';
import { renderColors } from './preview/tokens-colors';

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
        { selected: state.selected },
        state.catalog,
        (id: string) => { state.selected = id; },
      )}
      <main class="kit-work">
        ${() => routeWork(state.selected, state.catalog)}
      </main>
    </div>
  `(target);
}

function routeWork(selected: string, catalog: CatalogEntry[]) {
  if (selected === 'tokens.colors') return renderColors(catalog);
  return html`<div class="kit-placeholder">${selected}</div>`;
}
