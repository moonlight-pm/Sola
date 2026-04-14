import { on, invoke } from '@sola/ipc';

const appNameEl = document.getElementById('app-name')!;
const menuLabelsEl = document.getElementById('menu-labels')!;
const clockEl = document.getElementById('clock')!;

let currentMenuLabels: string[] = [];

on('focus', (msg: any) => {
    appNameEl.textContent = msg.app_id || '';
    currentMenuLabels = msg.menu_labels || [];
    renderMenuLabels();
});

function renderMenuLabels(): void {
    while (menuLabelsEl.firstChild) menuLabelsEl.removeChild(menuLabelsEl.firstChild);

    currentMenuLabels.forEach((label: string, index: number) => {
        // Skip first label (app menu) in the label bar — it's shown via app-name
        if (index === 0) return;

        const el = document.createElement('div');
        el.className = 'menu-label';
        el.textContent = label;
        el.addEventListener('click', () => {
            invoke('menu_click', { menu_index: index });
        });
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
