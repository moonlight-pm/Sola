import { on } from '@sola/ipc';

const systemMenuEl = document.getElementById('system-menu')!;
const appNameEl = document.getElementById('app-name')!;
const menuLabelsEl = document.getElementById('menu-labels')!;
const clockEl = document.getElementById('clock')!;

let currentMenuLabels: string[] = [];
let systemMenuOpen = false;

on('focus', (msg: any) => {
    appNameEl.textContent = msg.app_name || '';
    currentMenuLabels = msg.menu_labels || [];
    renderMenuLabels();
});

// System menu (eclipse icon)
systemMenuEl.addEventListener('click', () => {
    systemMenuOpen = !systemMenuOpen;
    if (systemMenuOpen) {
        document.title = 'cmd:system_menu';
    }
});

function renderMenuLabels(): void {
    while (menuLabelsEl.firstChild) menuLabelsEl.removeChild(menuLabelsEl.firstChild);

    currentMenuLabels.forEach((label: string, index: number) => {
        if (index === 0) return;

        const el = document.createElement('div');
        el.className = 'menu-label';
        el.textContent = label;
        menuLabelsEl.appendChild(el);
    });
}

// Clock
function updateClock(): void {
    const now = new Date();
    const h = now.getHours();
    const m = String(now.getMinutes()).padStart(2, '0');
    const ampm = h >= 12 ? 'PM' : 'AM';
    const h12 = h % 12 || 12;
    clockEl.textContent = `${h12}:${m} ${ampm}`;
}

updateClock();
setInterval(updateClock, 10000);
