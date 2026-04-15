import { invoke } from '@sola/ipc';

const menuEl = document.getElementById('menu')!;

document.addEventListener('click', (e: Event) => {
    if (!menuEl.contains(e.target as Node)) {
        invoke('dismiss', {});
    }
});

// Dismiss the menu when the pointer leaves the dropdown area.
menuEl.addEventListener('mouseleave', () => {
    invoke('dismiss', {});
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
            // Rust produces the final Mac-style display string (e.g. "⌘T");
            // we just render it.
            sc.textContent = item.shortcut;
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

// Rust calls these via evaluate_javascript — must be global.
(window as any).showMenu = showMenu;
(window as any).clearMenu = clearMenu;
