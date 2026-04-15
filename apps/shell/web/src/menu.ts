import { invoke } from '@sola/ipc';

const menuEl = document.getElementById('menu')!;

document.addEventListener('click', (e: Event) => {
    if (!menuEl.contains(e.target as Node)) {
        invoke('dismiss', {});
    }
});

function showMenu(items: any[], anchorX: number): void {
    while (menuEl.firstChild) menuEl.removeChild(menuEl.firstChild);
    menuEl.style.left = (anchorX || 0) + 'px';
    menuEl.style.top = '0px';
    menuEl.className = 'active';

    items.forEach((item: any) => {
        if (item.type === 'divider') {
            const div = document.createElement('div');
            div.className = 'menu-divider';
            menuEl.appendChild(div);
            return;
        }

        const el = document.createElement('div');
        el.className = 'menu-item' + (item.disabled ? ' disabled' : '');

        const label = document.createElement('span');
        label.textContent = item.label;
        el.appendChild(label);

        if (item.shortcut) {
            const sc = document.createElement('span');
            sc.className = 'shortcut';
            sc.textContent = formatShortcut(item.shortcut);
            el.appendChild(sc);
        }

        if (!item.disabled) {
            el.addEventListener('click', () => {
                invoke('action', { app_id: item.app_id, action_id: item.id });
            });
        }

        menuEl.appendChild(el);
    });
}

function clearMenu(): void {
    menuEl.className = '';
    while (menuEl.firstChild) menuEl.removeChild(menuEl.firstChild);
}

function formatShortcut(s: string): string {
    return s
        .replace(/Super\+Shift\+/g, '\u21E7\u2318')
        .replace(/Super\+/g, '\u2318')
        .replace(/Shift\+/g, '\u21E7')
        .replace('Backspace', '\u232B');
}

// Rust calls these via evaluate_javascript — must be global.
(window as any).showMenu = showMenu;
(window as any).clearMenu = clearMenu;
