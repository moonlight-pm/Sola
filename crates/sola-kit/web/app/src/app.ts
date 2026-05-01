import { html, reactive } from '@arrow-js/core';
import { renderSidebar, type CatalogEntry } from './sidebar';

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
        <div class="kit-placeholder">${() => state.selected}</div>
      </main>
    </div>
  `(target);
}
