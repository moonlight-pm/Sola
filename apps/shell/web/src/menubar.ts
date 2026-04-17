import { invoke, on } from '@sola/ipc';

const systemMenuEl = document.getElementById('system-menu')!;
const appNameEl = document.getElementById('app-name')!;
const menuLabelsEl = document.getElementById('menu-labels')!;
const clockEl = document.getElementById('clock')!;
const toastEl = document.getElementById('toast')!;

let currentMenuLabels: string[] = [];
let openKey: string | null = null;

on('focus', (msg: any) => {
    appNameEl.textContent = msg.app_name || '';
    currentMenuLabels = msg.menu_labels || [];
    dismissMenu();
    renderMenuLabels();
});

on('close_menu', () => {
    openKey = null;
    updateActiveState();
});

let toastTimer: number | null = null;
on('toast', (msg: any) => {
    toastEl.textContent = msg.message || '';
    toastEl.classList.add('visible');
    if (toastTimer !== null) {
        clearTimeout(toastTimer);
    }
    toastTimer = window.setTimeout(() => {
        toastEl.classList.remove('visible');
        toastTimer = null;
    }, 5000);
});

function clickMenu(key: string, source: string, index: number, anchorX: number): void {
    if (openKey === key) {
        dismissMenu();
        return;
    }
    showMenu(key, source, index, anchorX);
}

function hoverMenu(key: string, source: string, index: number, anchorX: number): void {
    if (openKey === null || openKey === key) return;
    showMenu(key, source, index, anchorX);
}

function showMenu(key: string, source: string, index: number, anchorX: number): void {
    openKey = key;
    updateActiveState();
    invoke('open_menu', { source, index, anchor_x: anchorX });
}

function dismissMenu(): void {
    if (openKey === null) return;
    openKey = null;
    updateActiveState();
    invoke('close_menu', {});
}

function updateActiveState(): void {
    systemMenuEl.classList.toggle('active', openKey === 'system');
    appNameEl.classList.toggle('active', openKey === 'app:0');
    const labels = menuLabelsEl.children;
    for (let i = 0; i < labels.length; i++) {
        labels[i].classList.toggle('active', openKey === 'app:' + (i + 1));
    }
}

systemMenuEl.addEventListener('click', (e: Event) => {
    e.stopPropagation();
    clickMenu('system', 'system', 0, systemMenuEl.getBoundingClientRect().left);
});

systemMenuEl.addEventListener('mouseenter', () => {
    hoverMenu('system', 'system', 0, systemMenuEl.getBoundingClientRect().left);
});

appNameEl.addEventListener('click', (e: Event) => {
    e.stopPropagation();
    if (currentMenuLabels.length === 0) return;
    clickMenu('app:0', 'app', 0, appNameEl.getBoundingClientRect().left);
});

appNameEl.addEventListener('mouseenter', () => {
    if (currentMenuLabels.length === 0) return;
    hoverMenu('app:0', 'app', 0, appNameEl.getBoundingClientRect().left);
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
            clickMenu('app:' + index, 'app', index, el.getBoundingClientRect().left);
        });
        el.addEventListener('mouseenter', () => {
            hoverMenu('app:' + index, 'app', index, el.getBoundingClientRect().left);
        });
        menuLabelsEl.appendChild(el);
    });
}

document.addEventListener('click', () => {
    dismissMenu();
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
