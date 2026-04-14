import { on } from '@sola/ipc';

const container = document.getElementById('container')!;
let apps: any[] = [];
let selectedIndex = 0;

on('render', (msg: any) => {
    apps = JSON.parse(msg.apps);
    selectedIndex = msg.selected;
    renderApps();
});

function renderApps(): void {
    while (container.firstChild) container.removeChild(container.firstChild);

    apps.forEach((app: any, i: number) => {
        const el = document.createElement('div');
        el.className = 'app' + (i === selectedIndex ? ' selected' : '');

        const iconEl = document.createElement('div');
        iconEl.className = 'icon';
        iconEl.textContent = '\u2B21';
        el.appendChild(iconEl);

        const nameEl = document.createElement('div');
        nameEl.className = 'name';
        nameEl.textContent = app.name;
        el.appendChild(nameEl);

        el.addEventListener('mouseenter', () => {
            setSelection(i);
        });

        container.appendChild(el);
    });
}

function setSelection(index: number): void {
    if (index < 0 || index >= apps.length) return;
    selectedIndex = index;
    document.title = String(index);
    const children = container.children;
    for (let i = 0; i < children.length; i++) {
        children[i].classList.toggle('selected', i === selectedIndex);
    }
}

function clear(): void {
    apps = [];
    selectedIndex = 0;
    while (container.firstChild) container.removeChild(container.firstChild);
}

// Expose to Rust's evaluate_javascript calls
(window as any).setSelection = setSelection;
(window as any).clear = clear;
