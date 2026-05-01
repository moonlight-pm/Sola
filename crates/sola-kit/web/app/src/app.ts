import { html, reactive } from '@arrow-js/core';
import { renderSidebar, type CatalogEntry } from './sidebar';
import { renderColors } from './preview/tokens-colors';
import { renderTypography } from './preview/tokens-typography';
import { renderSpacing } from './preview/tokens-spacing';
import { renderComponent } from './preview/component-view';

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
  if (selected === 'tokens.colors')     return renderColors(catalog);
  if (selected === 'tokens.typography') return renderTypography();
  if (selected === 'tokens.spacing')    return renderSpacing();
  if (selected.startsWith('atoms.'))     return renderComponent(selected.slice('atoms.'.length), catalog);
  if (selected.startsWith('components.')) return renderComponent(selected.slice('components.'.length), catalog);
  return html`<div class="kit-placeholder">${selected}</div>`;
}
