// Application launcher overlay. Rust drives rendering via renderApps() and
// setSelection(); the input dispatches cmd messages back through @sola/ipc.

import { invoke } from '@sola/ipc';

type Entry = { app_id: string; label: string; icon: string };

const queryEl = document.getElementById('query') as HTMLInputElement;
const resultsEl = document.getElementById('results')!;

let entries: Entry[] = [];
let selected = 0;

function renderApps(list: Entry[], sel: number) {
    entries = list;
    selected = sel;

    while (resultsEl.firstChild) resultsEl.removeChild(resultsEl.firstChild);

    if (entries.length === 0) {
        const empty = document.createElement('div');
        empty.id = 'empty';
        empty.textContent = 'No matching applications.';
        resultsEl.appendChild(empty);
        return;
    }

    entries.forEach((app, i) => {
        const row = document.createElement('div');
        row.className = 'row' + (i === selected ? ' selected' : '');

        const iconEl = document.createElement('div');
        iconEl.className = 'icon';
        if (app.icon && app.icon.indexOf('/') > 0) {
            const img = document.createElement('img');
            img.src = 'sola-assets://icons/' + app.icon + '.svg';
            iconEl.appendChild(img);
        } else {
            iconEl.textContent = '\u2B21';
        }
        row.appendChild(iconEl);

        const labelEl = document.createElement('span');
        labelEl.className = 'label';
        labelEl.textContent = app.label;
        row.appendChild(labelEl);

        row.addEventListener('mouseenter', () => setSelection(i));
        row.addEventListener('click', () => launchSelected(i));

        resultsEl.appendChild(row);
    });
}

function setSelection(i: number) {
    if (i < 0 || i >= entries.length) return;
    selected = i;
    const rows = resultsEl.children;
    for (let k = 0; k < rows.length; k++) {
        rows[k].classList.toggle('selected', k === selected);
    }
    const active = rows[selected] as HTMLElement | undefined;
    active?.scrollIntoView({ block: 'nearest' });
}

function launchSelected(i?: number) {
    const idx = typeof i === 'number' ? i : selected;
    const app = entries[idx];
    if (!app) return;
    invoke('launch', { app_id: app.app_id });
}

function close() {
    invoke('close', {});
}

function resetForOpen() {
    queryEl.value = '';
    queryEl.focus();
}

queryEl.addEventListener('input', () => {
    invoke('query', { text: queryEl.value });
});

queryEl.addEventListener('keydown', (e) => {
    switch (e.key) {
        case 'ArrowDown':
            e.preventDefault();
            setSelection(Math.min(selected + 1, entries.length - 1));
            break;
        case 'ArrowUp':
            e.preventDefault();
            setSelection(Math.max(selected - 1, 0));
            break;
        case 'Enter':
            e.preventDefault();
            launchSelected();
            break;
        case 'Escape':
            e.preventDefault();
            close();
            break;
    }
});

(window as any).renderApps = renderApps;
(window as any).setSelection = setSelection;
(window as any).resetForOpen = resetForOpen;
