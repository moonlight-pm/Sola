import { invoke, on } from '@sola/ipc';

const systemMenuEl = document.getElementById('system-menu')!;
const appNameEl = document.getElementById('app-name')!;
const menuLabelsEl = document.getElementById('menu-labels')!;
const clockEl = document.getElementById('clock')!;

let currentMenuLabels: string[] = [];
let openIndex: number | null = null;

on('focus', (msg: any) => {
    appNameEl.textContent = msg.app_name || '';
    currentMenuLabels = msg.menu_labels || [];
    closeMenu();
    renderMenuLabels();
});

on('close_menu', () => {
    closeMenu();
});

function openMenu(index: number): void {
    if (openIndex === index) {
        closeMenu();
        return;
    }
    openIndex = index;
    updateActiveState();
    invoke('open_menu', { index });
}

function closeMenu(): void {
    if (openIndex === null) return;
    openIndex = null;
    updateActiveState();
    invoke('close_menu', {});
}

function updateActiveState(): void {
    systemMenuEl.classList.toggle('active', openIndex === 0);
    const labels = menuLabelsEl.children;
    for (let i = 0; i < labels.length; i++) {
        const labelIndex = i + 1;
        labels[i].classList.toggle('active', openIndex === labelIndex);
    }
}

systemMenuEl.addEventListener('click', (e: Event) => {
    e.stopPropagation();
    openMenu(0);
});

systemMenuEl.addEventListener('mouseenter', () => {
    if (openIndex !== null) openMenu(0);
});

function renderMenuLabels(): void {
    while (menuLabelsEl.firstChild) menuLabelsEl.removeChild(menuLabelsEl.firstChild);

    currentMenuLabels.forEach((label: string, index: number) => {
        if (index === 0) return;

        const el = document.createElement('div');
        el.className = 'menu-label';
        el.textContent = label;
        el.addEventListener('click', (e: Event) => {
            e.stopPropagation();
            openMenu(index);
        });
        el.addEventListener('mouseenter', () => {
            if (openIndex !== null) openMenu(index);
        });
        menuLabelsEl.appendChild(el);
    });
}

document.addEventListener('click', () => {
    closeMenu();
});

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
