// Application launcher overlay. Rust is authoritative for `entries` and
// `selected`; it pushes both via `renderApps(list, selected)`. Arrow.js
// re-renders the results reactively from the store.

import { html } from '@arrow-js/core';
import { invoke } from '@sola/ipc';
import { createStore } from '@sola/store';

type Entry = { app_id: string; label: string; icon: string };

const state = createStore({
    entries: [] as Entry[],
    selected: 0,
});

const queryEl = document.getElementById('query') as HTMLInputElement;
const resultsEl = document.getElementById('results')!;
const panelEl = document.getElementById('panel')!;

// Click anywhere outside the panel dismisses the launcher. Because the
// window spans the output, this also absorbs all pointer activity away
// from whatever's underneath.
document.addEventListener('mousedown', (e: MouseEvent) => {
    if (!panelEl.contains(e.target as Node)) {
        invoke('close', {});
    }
});

function launchAt(index: number): void {
    const entry = state.entries[index];
    if (!entry) return;
    invoke('launch', { app_id: entry.app_id });
}

function launchSelected(): void {
    invoke('launch', {});
}

function navDir(dir: 'up' | 'down'): void {
    invoke('nav', { dir });
}

function navTo(index: number): void {
    invoke('nav', { index });
}

html`
    ${() =>
        state.entries.length === 0
            ? html`<div id="empty">No matching applications.</div>`
            : state.entries.map(
                  (app, i) => html`
                      <div
                          class="${() =>
                              'row' + (i === state.selected ? ' selected' : '')}"
                          @mouseenter="${() => navTo(i)}"
                          @click="${() => launchAt(i)}"
                      >
                          <div class="icon">
                              ${() =>
                                  app.icon && app.icon.indexOf('/') > 0
                                      ? html`<img
                                            src="${'sola-assets://icons/' +
                                            app.icon +
                                            '.svg'}"
                                        />`
                                      : html`<span>${'\u2B21'}</span>`}
                          </div>
                          <span class="label">${app.label}</span>
                      </div>
                  `,
              )}
`(resultsEl);

queryEl.addEventListener('input', () => {
    invoke('query', { text: queryEl.value });
});

queryEl.addEventListener('keydown', (e) => {
    switch (e.key) {
        case 'ArrowDown':
            e.preventDefault();
            navDir('down');
            break;
        case 'ArrowUp':
            e.preventDefault();
            navDir('up');
            break;
        case 'Enter':
            e.preventDefault();
            launchSelected();
            break;
        case 'Escape':
            e.preventDefault();
            invoke('close', {});
            break;
    }
});

// Rust pushes the full list + selection index on every state change.
// Scroll the new selection into view after the Arrow.js render tick.
function renderApps(list: Entry[], sel: number): void {
    state.entries = list;
    state.selected = sel;
    queueMicrotask(() => {
        const active = resultsEl.children[sel] as HTMLElement | undefined;
        active?.scrollIntoView({ block: 'nearest' });
    });
}

function resetForOpen(): void {
    queryEl.value = '';
    queryEl.focus();
}

(window as any).renderApps = renderApps;
(window as any).resetForOpen = resetForOpen;
